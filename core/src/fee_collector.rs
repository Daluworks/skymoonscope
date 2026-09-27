use crate::fee_store::{FeeStore, LedgerFeeSample};
use crate::leader_lock::RedisLeaderLock;
use crate::rpc_provider::ProviderRegistry;
use crate::sys_alarms::{emit_sys_alarm, SysAlarmConfig, SysAlarmEvent, ENV_INSTANCE_ID};
use crate::AppMetrics;
use chrono::Utc;
use reqwest::Client;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing;

/// Errors that can occur during fee collection
#[derive(Error, Debug)]
pub enum FeeCollectorError {
    #[error("RPC request failed: {0}")]
    RpcRequestFailed(String),

    #[error("RPC node timeout")]
    NodeTimeout,

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Store error: {0}")]
    StoreError(String),

    #[error("No healthy providers available")]
    NoHealthyProviders,
}

/// Configuration for the fee collector
#[derive(Debug, Clone)]
pub struct FeeCollectorConfig {
    /// How often to collect fee data (in seconds)
    pub collection_interval_secs: u64,
    /// How many ledgers to fetch in one batch
    pub batch_size: u64,
    /// Request timeout
    pub request_timeout: Duration,
}

impl Default for FeeCollectorConfig {
    fn default() -> Self {
        Self {
            collection_interval_secs: 5, // Stellar ledgers close every ~5 seconds
            batch_size: 10,
            request_timeout: Duration::from_secs(10),
        }
    }
}

/// Default number of attempts (including the first) for a fee-record write.
pub const DEFAULT_MAX_WRITE_ATTEMPTS: usize = 3;
/// Default delay before the first retry.
pub const DEFAULT_WRITE_INITIAL_BACKOFF: Duration = Duration::from_millis(50);
/// Default upper bound for the exponential backoff between retries.
pub const DEFAULT_WRITE_MAX_BACKOFF: Duration = Duration::from_millis(500);
/// Default cap on the number of records held in the dead-letter buffer.
pub const DEFAULT_DEAD_LETTER_CAPACITY: usize = 256;
/// Default number of consecutive exhausted writes before an alarm is raised.
pub const DEFAULT_ALARM_AFTER_CONSECUTIVE_FAILURES: u64 = 5;

/// Tuning for the bounded retry + dead-letter behaviour around
/// fee-record persistence.
///
/// [`retry_with_backoff`] makes up to `max_attempts` attempts in total
/// (the first try counts) and sleeps `initial_backoff`,
/// `initial_backoff * backoff_multiplier`, … between attempts, capped at
/// `max_backoff`. Records that still fail are pushed onto the bounded
/// dead-letter buffer instead of disappearing.
#[derive(Debug, Clone)]
pub struct FeeWritePolicy {
    /// Total attempts per write, including the initial one. Clamped to >= 1.
    pub max_attempts: usize,
    /// Delay before the second attempt.
    pub initial_backoff: Duration,
    /// Multiplier applied to the inter-attempt delay after each failure.
    pub backoff_multiplier: u32,
    /// Upper bound for the inter-attempt delay.
    pub max_backoff: Duration,
    /// Maximum number of records retained in the dead-letter buffer.
    pub dead_letter_capacity: usize,
    /// Consecutive exhausted writes that trigger a `sys_alarms` alert.
    /// `0` disables alerting.
    pub alarm_after_consecutive_failures: u64,
}

impl Default for FeeWritePolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_WRITE_ATTEMPTS,
            initial_backoff: DEFAULT_WRITE_INITIAL_BACKOFF,
            backoff_multiplier: 2,
            max_backoff: DEFAULT_WRITE_MAX_BACKOFF,
            dead_letter_capacity: DEFAULT_DEAD_LETTER_CAPACITY,
            alarm_after_consecutive_failures: DEFAULT_ALARM_AFTER_CONSECUTIVE_FAILURES,
        }
    }
}

impl FeeWritePolicy {
    /// Next backoff delay, growing geometrically and clamped to
    /// [`Self::max_backoff`].
    fn next_backoff(&self, current: Duration) -> Duration {
        current
            .saturating_mul(self.backoff_multiplier.max(1))
            .min(self.max_backoff)
    }
}

/// A fee sample that could not be persisted after every retry attempt.
///
/// Retained in the collector's in-memory dead-letter buffer so operators
/// can inspect or drain it instead of losing the record silently. The
/// buffer is process-local and bounded by
/// [`FeeWritePolicy::dead_letter_capacity`].
#[derive(Debug, Clone)]
pub struct DeadLetterRecord {
    /// The fee sample that failed to persist.
    pub sample: LedgerFeeSample,
    /// Number of write attempts made before giving up.
    pub attempts: usize,
    /// Error returned by the final attempt.
    pub last_error: String,
    /// When the record was dead-lettered.
    pub enqueued_at: chrono::DateTime<Utc>,
}

/// Run `operation` with bounded exponential backoff.
///
/// Returns the number of attempts used on success, or
/// `Err((attempts, last_error))` once `policy.max_attempts` is reached.
/// `operation` is always invoked at least once.
async fn retry_with_backoff<F, Fut>(
    policy: &FeeWritePolicy,
    mut operation: F,
) -> Result<usize, (usize, String)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let max_attempts = policy.max_attempts.max(1);
    let mut backoff = policy.initial_backoff;
    let mut last_error = String::new();

    for attempt in 1..=max_attempts {
        match operation().await {
            Ok(()) => return Ok(attempt),
            Err(err) => {
                last_error = err;
                if attempt < max_attempts {
                    tracing::warn!(
                        attempt,
                        max_attempts,
                        backoff_ms = backoff.as_millis() as u64,
                        error = %last_error,
                        "Fee record DB write failed; backing off before retry"
                    );
                    if !backoff.is_zero() {
                        tokio::time::sleep(backoff).await;
                    }
                    backoff = policy.next_backoff(backoff);
                }
            }
        }
    }

    Err((max_attempts, last_error))
}

/// Fee collector that polls RPC nodes for ledger fee data
pub struct FeeCollector {
    registry: Arc<ProviderRegistry>,
    store: Arc<FeeStore>,
    client: Client,
    config: FeeCollectorConfig,
    last_collected_sequence: std::sync::atomic::AtomicU64,
    metrics: Arc<AppMetrics>,
    /// Leader-election lease. Only the instance holding it persists ledger
    /// fee samples, so running multiple Core instances doesn't cause
    /// duplicate collection or racing writes to `FeeStore`.
    leader_lock: Arc<RedisLeaderLock>,
    /// Bounded retry / dead-letter tuning for fee-record writes.
    write_policy: FeeWritePolicy,
    /// `sys_alarms` configuration used to alert when writes are
    /// dead-lettered.
    alarm_config: SysAlarmConfig,
    /// Process-local buffer of fee samples that exhausted their retries,
    /// bounded by [`FeeWritePolicy::dead_letter_capacity`].
    dead_letters: Mutex<VecDeque<DeadLetterRecord>>,
    /// Count of records evicted because the dead-letter buffer was full.
    dead_letters_dropped: AtomicU64,
    /// Consecutive fee-record writes that exhausted their retries.
    consecutive_store_failures: AtomicU64,
    /// Count of `sys_alarms` alerts raised by the dead-letter path.
    alarms_raised: AtomicU64,
}

impl FeeCollector {
    /// Create a new fee collector
    pub fn new(
        registry: Arc<ProviderRegistry>,
        store: Arc<FeeStore>,
        config: FeeCollectorConfig,
        metrics: Arc<AppMetrics>,
        leader_lock: Arc<RedisLeaderLock>,
    ) -> Self {
        Self {
            registry,
            store,
            client: Client::builder()
                .timeout(config.request_timeout)
                .build()
                .expect("Failed to create HTTP client"),
            config,
            last_collected_sequence: std::sync::atomic::AtomicU64::new(0),
            write_policy: FeeWritePolicy::default(),
            alarm_config: SysAlarmConfig::from_env(),
            dead_letters: Mutex::new(VecDeque::new()),
            dead_letters_dropped: AtomicU64::new(0),
            consecutive_store_failures: AtomicU64::new(0),
            alarms_raised: AtomicU64::new(0),
            metrics,
            leader_lock,
        }
    }

    /// Override the bounded-retry / dead-letter policy (defaults to
    /// [`FeeWritePolicy::default`]).
    pub fn with_write_policy(mut self, policy: FeeWritePolicy) -> Self {
        self.write_policy = policy;
        self
    }

    /// Override the `sys_alarms` configuration used for dead-letter
    /// alerts (defaults to [`SysAlarmConfig::from_env`]).
    pub fn with_alarm_config(mut self, alarm_config: SysAlarmConfig) -> Self {
        self.alarm_config = alarm_config;
        self
    }

    /// Persist `sample`, retrying transient failures with backoff and
    /// dead-lettering it when every attempt fails.
    ///
    /// On success the consecutive-failure counter is reset and `Ok(())`
    /// is returned. On exhaustion the sample is copied into the bounded
    /// dead-letter buffer, a `sys_alarms` alert is raised once the
    /// consecutive-failure threshold is crossed, and the final store
    /// error is returned so callers keep logging / counting it — errors
    /// are never swallowed.
    async fn persist_sample_with_retry(
        &self,
        sample: &LedgerFeeSample,
    ) -> Result<(), FeeCollectorError> {
        let store = Arc::clone(&self.store);
        let ledger = sample.ledger_sequence;
        let mut attempt = 0usize;

        let outcome = retry_with_backoff(&self.write_policy, || {
            let store = Arc::clone(&store);
            attempt += 1;
            let attempt_no = attempt;
            async move {
                store
                    .upsert_ledger_sample(sample)
                    .await
                    .map_err(|e| e.to_string())
            }
        })
        .await;

        match outcome {
            Ok(attempts) => {
                self.note_store_success();
                if attempts > 1 {
                    tracing::info!(ledger, attempts, "Fee record persisted after retry");
                }
                Ok(())
            }
            Err((attempts, error)) => {
                tracing::error!(
                    ledger,
                    attempts,
                    error = %error,
                    "Fee record DB write failed after all retries; dead-lettering"
                );
                self.enqueue_dead_letter(sample, attempts, &error).await;
                Err(FeeCollectorError::StoreError(error))
            }
        }
    }

    /// Reset the consecutive-failure counter after a successful write.
    fn note_store_success(&self) {
        self.consecutive_store_failures
            .store(0, Ordering::Relaxed);
    }

    /// Push a permanently-failed sample onto the bounded dead-letter
    /// buffer and raise a `sys_alarms` alert when the consecutive-failure
    /// threshold is crossed.
    async fn enqueue_dead_letter(
        &self,
        sample: &LedgerFeeSample,
        attempts: usize,
        last_error: &str,
    ) {
        let record = DeadLetterRecord {
            sample: sample.clone(),
            attempts,
            last_error: last_error.to_string(),
            enqueued_at: Utc::now(),
        };

        let capacity = self.write_policy.dead_letter_capacity;
        if capacity == 0 {
            self.dead_letters_dropped.fetch_add(1, Ordering::Relaxed);
        } else {
            let mut buffer = self
                .dead_letters
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            while buffer.len() >= capacity {
                buffer.pop_front();
                self.dead_letters_dropped.fetch_add(1, Ordering::Relaxed);
            }
            buffer.push_back(record);
        }

        let consecutive = self
            .consecutive_store_failures
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        let threshold = self.write_policy.alarm_after_consecutive_failures;
        // Alert on the first crossing and then once per further threshold
        // worth of failures, so a sustained outage does not page per record.
        if threshold > 0 && (consecutive == threshold || consecutive % threshold == 0) {
            self.raise_fee_store_alarm(consecutive).await;
        }
    }

    /// Emit a `sys_alarms` alert describing the consecutive write failures.
    ///
    /// [`SysAlarmEvent`] is reused as-is: `value_percent` carries the
    /// consecutive-failure count, `threshold_percent` the configured
    /// threshold, and `process_memory_bytes` / `total_memory_bytes` the
    /// current dead-letter depth / capacity.
    async fn raise_fee_store_alarm(&self, consecutive: u64) {
        self.alarms_raised.fetch_add(1, Ordering::Relaxed);
        let threshold = self.write_policy.alarm_after_consecutive_failures;
        tracing::warn!(
            consecutive_failures = consecutive,
            threshold,
            dead_letter_depth = self.dead_letter_len(),
            "Fee collector exceeded the consecutive DB write failure threshold"
        );

        if !self.alarm_config.enabled {
            tracing::warn!("sys_alarms is disabled; fee collector alert not emitted");
            return;
        }

        let node_id = std::env::var(ENV_INSTANCE_ID)
            .ok()
            .or_else(|| std::env::var("HOSTNAME").ok())
            .filter(|value| !value.trim().is_empty());

        let payload = SysAlarmEvent {
            event: "fee_store_write_failed",
            resource: "fee_collector",
            value_percent: consecutive as f64,
            threshold_percent: threshold as f64,
            process_memory_bytes: self.dead_letter_len() as u64,
            total_memory_bytes: self.write_policy.dead_letter_capacity as u64,
            pid: std::process::id(),
            node_id,
            timestamp: Utc::now(),
        };

        emit_sys_alarm(self.alarm_config.webhook_url.as_deref(), &payload).await;
    }

    /// Number of fee samples currently held in the dead-letter buffer.
    pub fn dead_letter_len(&self) -> usize {
        self.dead_letters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Configured maximum size of the dead-letter buffer.
    pub fn dead_letter_capacity(&self) -> usize {
        self.write_policy.dead_letter_capacity
    }

    /// Total number of records evicted from a full dead-letter buffer.
    pub fn dead_letters_dropped(&self) -> u64 {
        self.dead_letters_dropped.load(Ordering::Relaxed)
    }

    /// Snapshot of the dead-letter buffer, oldest first, without removing
    /// anything.
    pub fn dead_letter_snapshot(&self) -> Vec<DeadLetterRecord> {
        self.dead_letters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    /// Remove and return every buffered dead-letter record, oldest first,
    /// emptying the buffer.
    pub fn drain_dead_letters(&self) -> Vec<DeadLetterRecord> {
        self.dead_letters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain(..)
            .collect()
    }

    /// Consecutive fee-record writes that exhausted their retries. Reset
    /// to `0` by the next successful write.
    pub fn consecutive_store_failures(&self) -> u64 {
        self.consecutive_store_failures.load(Ordering::Relaxed)
    }

    /// Number of `sys_alarms` alerts raised by the dead-letter path.
    pub fn alarms_raised(&self) -> u64 {
        self.alarms_raised.load(Ordering::Relaxed)
    }

    /// Run the background collection loop until a shutdown signal is received.
    pub async fn run_collection_loop(
        self: Arc<Self>,
        mut shutdown: tokio::sync::broadcast::Receiver<()>,
    ) {
        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.collection_interval_secs));

        tracing::info!(
            interval_secs = self.config.collection_interval_secs,
            "Fee collector started"
        );

        loop {
            tokio::select! {
                biased;
                _ = shutdown.recv() => {
                    tracing::info!("Fee collector shutting down");
                    break;
                }
                _ = interval.tick() => {
                    if !self.leader_lock.try_acquire_or_renew().await {
                        tracing::debug!("not leader this cycle, skipping fee collection");
                        continue;
                    }

                    let started_at = Instant::now();
                    let result = self.collect_latest_fees().await;
                    self.metrics
                        .indexing_latency_seconds
                        .with_label_values(&["fee_collector"])
                        .observe(started_at.elapsed().as_secs_f64());

                    match result {
                        Ok(true) => {
                            self.metrics
                                .events_processed_total
                                .with_label_values(&["fee_collector"])
                                .inc();
                        }
                        Ok(false) => {}
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to collect fee data");
                            self.metrics
                                .indexing_errors_total
                                .with_label_values(&["fee_collector"])
                                .inc();
                        }
                    }
                }
            }
        }
    }

    /// Collect fee data from the latest ledger. Returns `true` when a new
    /// ledger's fee sample was fetched and persisted, `false` when there was
    /// nothing new to collect.
    async fn collect_latest_fees(&self) -> Result<bool, FeeCollectorError> {
        // Get latest ledger sequence
        let latest_sequence = self.get_latest_ledger_sequence().await?;

        let mut last_collected = self
            .last_collected_sequence
            .load(std::sync::atomic::Ordering::Relaxed);

        // If in-memory state is uninitialized, load from the database
        if last_collected == 0 {
            if let Ok(Some(db_latest)) = self.store.get_latest_sequence().await {
                last_collected = db_latest as u64;
                self.last_collected_sequence
                    .store(last_collected, std::sync::atomic::Ordering::Relaxed);
            }
        }

        // Skip if we've already collected this ledger
        if latest_sequence <= last_collected {
            tracing::debug!(
                latest = latest_sequence,
                last_collected = last_collected,
                "No new ledgers to collect"
            );
            return Ok(false);
        }

        // Trigger automatic catch-up replay loop upon reconnection / gap detection
        if last_collected > 0 && latest_sequence > last_collected + 1 {
            let start = last_collected + 1;
            let end = latest_sequence - 1;
            tracing::info!(
                start = start,
                end = end,
                "RPC Node re-synchronized. Catching up missed ledgers."
            );
            for seq in start..=end {
                match self.fetch_ledger_fee_data(seq).await {
                    Ok(sample) => {
                        if let Err(e) = self.persist_sample_with_retry(&sample).await {
                            tracing::error!(
                                ledger = seq,
                                error = %e,
                                "Failed to save catch-up ledger sample"
                            );
                            break;
                        }
                        self.last_collected_sequence
                            .store(seq, std::sync::atomic::Ordering::Relaxed);
                        tracing::info!(ledger = seq, "Successfully caught up missed ledger");
                    }
                    Err(e) => {
                        tracing::error!(
                            ledger = seq,
                            error = %e,
                            "Failed to fetch catch-up ledger details; stopping catch-up replay"
                        );
                        break;
                    }
                }
            }
        }

        // Fetch latest ledger details
        let sample = self.fetch_ledger_fee_data(latest_sequence).await?;

        // Store in database, retrying transient failures and dead-lettering
        // records that still fail so no fee sample is silently dropped.
        self.persist_sample_with_retry(&sample).await?;

        // Update last collected sequence
        self.last_collected_sequence
            .store(latest_sequence, std::sync::atomic::Ordering::Relaxed);

        tracing::info!(
            ledger = latest_sequence,
            base_fee = sample.base_fee,
            transaction_count = sample.transaction_count,
            "Collected fee data"
        );

        Ok(true)
    }

    /// Get the latest ledger sequence from RPC
    async fn get_latest_ledger_sequence(&self) -> Result<u64, FeeCollectorError> {
        let providers = self.registry.healthy_providers().await;
        if providers.is_empty() {
            return Err(FeeCollectorError::NoHealthyProviders);
        }

        // Try the first healthy provider
        let provider = &providers[0];

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestLedger",
            "params": null
        });

        let mut req = self.client.post(&provider.url).json(&body);

        // Attach auth headers if present
        if let (Some(header), Some(value)) = (&provider.auth_header, &provider.auth_value) {
            req = req.header(header.as_str(), value.as_str());
        }

        let response = req
            .send()
            .await
            .map_err(|e| FeeCollectorError::RpcRequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(FeeCollectorError::RpcRequestFailed(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| FeeCollectorError::ParseError(e.to_string()))?;

        let sequence = json["result"]["sequence"].as_u64().ok_or_else(|| {
            FeeCollectorError::ParseError("Missing sequence in response".to_string())
        })?;

        Ok(sequence)
    }

    /// Fetch detailed fee data for a specific ledger
    async fn fetch_ledger_fee_data(
        &self,
        sequence: u64,
    ) -> Result<LedgerFeeSample, FeeCollectorError> {
        let providers = self.registry.healthy_providers().await;
        if providers.is_empty() {
            return Err(FeeCollectorError::NoHealthyProviders);
        }

        let provider = &providers[0];

        // Try getLedgers endpoint first (if available)
        // Fallback to parsing from getTransactions if needed
        match self.fetch_from_get_ledgers(provider, sequence).await {
            Ok(sample) => Ok(sample),
            Err(e) => {
                tracing::warn!(error = %e, "getLedgers not available, using fallback");
                self.fetch_from_get_transactions(provider, sequence).await
            }
        }
    }

    /// Fetch fee data using getLedgers RPC method
    async fn fetch_from_get_ledgers(
        &self,
        provider: &crate::rpc_provider::RpcProvider,
        sequence: u64,
    ) -> Result<LedgerFeeSample, FeeCollectorError> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLedgers",
            "params": {
                "startLedger": sequence,
                "limit": 1
            }
        });

        let mut req = self.client.post(&provider.url).json(&body);

        if let (Some(header), Some(value)) = (&provider.auth_header, &provider.auth_value) {
            req = req.header(header.as_str(), value.as_str());
        }

        let response = req
            .send()
            .await
            .map_err(|e| FeeCollectorError::RpcRequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(FeeCollectorError::RpcRequestFailed(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| FeeCollectorError::ParseError(e.to_string()))?;

        // Parse ledger data from response
        let ledgers = json["result"]["ledgers"]
            .as_array()
            .ok_or_else(|| FeeCollectorError::ParseError("Missing ledgers array".to_string()))?;

        if ledgers.is_empty() {
            return Err(FeeCollectorError::ParseError(
                "No ledger data returned".to_string(),
            ));
        }

        let ledger = &ledgers[0];
        self.parse_ledger_sample(sequence, ledger)
    }

    /// Fallback: fetch fee data using getTransactions
    async fn fetch_from_get_transactions(
        &self,
        provider: &crate::rpc_provider::RpcProvider,
        sequence: u64,
    ) -> Result<LedgerFeeSample, FeeCollectorError> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransactions",
            "params": {
                "startLedger": sequence,
                "limit": 100
            }
        });

        let mut req = self.client.post(&provider.url).json(&body);

        if let (Some(header), Some(value)) = (&provider.auth_header, &provider.auth_value) {
            req = req.header(header.as_str(), value.as_str());
        }

        let response = req
            .send()
            .await
            .map_err(|e| FeeCollectorError::RpcRequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(FeeCollectorError::RpcRequestFailed(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| FeeCollectorError::ParseError(e.to_string()))?;

        let transactions = json["result"]["transactions"].as_array().ok_or_else(|| {
            FeeCollectorError::ParseError("Missing transactions array".to_string())
        })?;

        // Calculate fee statistics from transactions
        let mut total_fee_charged: i64 = 0;
        let mut max_fee: i64 = 0;
        let mut tx_count: i32 = 0;

        for tx in transactions {
            if let Some(fee_charged) = tx["feeCharged"].as_str() {
                if let Ok(fee) = fee_charged.parse::<i64>() {
                    total_fee_charged += fee;
                    max_fee = max_fee.max(fee);
                    tx_count += 1;
                }
            }
        }

        let avg_fee = if tx_count > 0 {
            total_fee_charged / tx_count as i64
        } else {
            100 // Default base fee
        };

        Ok(LedgerFeeSample {
            ledger_sequence: sequence as i64,
            collected_at: Utc::now(),
            base_reserve: 0, // Not available from this endpoint
            base_fee: avg_fee,
            max_fee: if max_fee > 0 { max_fee } else { avg_fee },
            fee_charged: total_fee_charged,
            transaction_count: tx_count,
            ledger_close_time: Utc::now(),
        })
    }

    /// Parse ledger sample from getLedgers response
    fn parse_ledger_sample(
        &self,
        sequence: u64,
        ledger: &serde_json::Value,
    ) -> Result<LedgerFeeSample, FeeCollectorError> {
        let base_fee = ledger["header"]["baseFee"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(100); // Default base fee

        let base_reserve = ledger["header"]["baseReserve"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        let tx_count = ledger["header"]["txSetSize"].as_u64().unwrap_or(0) as i32;

        // Parse close time
        let close_time_str = ledger["header"]["closeTime"].as_str().unwrap_or("0");

        let close_timestamp = close_time_str.parse::<i64>().unwrap_or(0);

        let ledger_close_time = if close_timestamp > 0 {
            chrono::DateTime::from_timestamp(close_timestamp, 0).unwrap_or_else(Utc::now)
        } else {
            Utc::now()
        };

        Ok(LedgerFeeSample {
            ledger_sequence: sequence as i64,
            collected_at: Utc::now(),
            base_reserve,
            base_fee,
            max_fee: base_fee, // Will be updated from transaction data
            fee_charged: 0,    // Will be calculated from transactions
            transaction_count: tx_count,
            ledger_close_time,
        })
    }

    /// Get the last collected sequence
    pub fn get_last_collected(&self) -> u64 {
        self.last_collected_sequence
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Fetch and store fee data for a single ledger sequence.
    ///
    /// Used by the `reindex` CLI subcommand to re-process historical ledgers.
    /// The data is upserted so re-running over the same range is idempotent.
    pub async fn fetch_and_store_ledger(&self, sequence: u64) -> Result<(), FeeCollectorError> {
        let sample = self.fetch_ledger_fee_data(sequence).await?;
        self.persist_sample_with_retry(&sample).await?;
        tracing::debug!(ledger = sequence, "Re-indexed ledger");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = FeeCollectorConfig::default();
        assert_eq!(config.collection_interval_secs, 5);
        assert_eq!(config.batch_size, 10);
        assert_eq!(config.request_timeout, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn collection_loop_exits_when_shutdown_is_broadcast() {
        let registry = ProviderRegistry::new(vec![crate::rpc_provider::RpcProvider {
            name: "test".to_string(),
            url: "http://127.0.0.1:9".to_string(),
            auth_header: None,
            auth_value: None,
            advertise: None,
        }]);

        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        let store = Arc::new(FeeStore::new(pool));
        let metrics = Arc::new(AppMetrics::new().expect("metrics"));
        let redis_client = redis::Client::open("redis://127.0.0.1:6379").unwrap();
        let leader_lock = Arc::new(RedisLeaderLock::new(
            redis_client,
            "test_fee_collector",
            Duration::from_secs(10),
        ));
        let collector = Arc::new(FeeCollector::new(
            registry,
            store,
            FeeCollectorConfig {
                collection_interval_secs: 60,
                batch_size: 1,
                request_timeout: Duration::from_millis(50),
            },
            metrics,
            leader_lock,
        ));

        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
        let handle = tokio::spawn(async move {
            collector.run_collection_loop(shutdown_rx).await;
        });

        // Give the loop a moment to start, then signal shutdown.
        tokio::time::sleep(Duration::from_millis(20)).await;
        shutdown_tx
            .send(())
            .expect("shutdown broadcast should succeed");

        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("fee collector should exit promptly after shutdown")
            .expect("fee collector task should not panic");
    }

    fn test_registry() -> Arc<ProviderRegistry> {
        Arc::new(ProviderRegistry::new(vec![crate::rpc_provider::RpcProvider {
            name: "test".to_string(),
            url: "http://127.0.0.1:9".to_string(),
            auth_header: None,
            auth_value: None,
            advertise: None,
        }]))
    }

    fn test_leader_lock() -> Arc<RedisLeaderLock> {
        let redis_client = redis::Client::open("redis://127.0.0.1:6379").unwrap();
        Arc::new(RedisLeaderLock::new(
            redis_client,
            "test_fee_collector_dead_letter",
            Duration::from_secs(10),
        ))
    }

    fn sample(ledger_sequence: i64) -> LedgerFeeSample {
        LedgerFeeSample {
            ledger_sequence,
            collected_at: Utc::now(),
            base_reserve: 0,
            base_fee: 100,
            max_fee: 100,
            fee_charged: 0,
            transaction_count: 0,
            ledger_close_time: Utc::now(),
        }
    }

    /// A store whose connection pool has been closed, so every write fails
    /// deterministically without touching a real database.
    async fn failing_store() -> Arc<FeeStore> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        pool.close().await;
        Arc::new(FeeStore::new(pool))
    }

    /// A store backed by an in-memory SQLite table that accepts writes.
    async fn writable_store() -> Arc<FeeStore> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        sqlx::query(
            "CREATE TABLE ledger_fee_samples (\
                ledger_sequence INTEGER PRIMARY KEY, \
                collected_at TEXT NOT NULL, \
                base_reserve INTEGER NOT NULL, \
                base_fee INTEGER NOT NULL, \
                max_fee INTEGER NOT NULL, \
                fee_charged INTEGER NOT NULL, \
                transaction_count INTEGER NOT NULL, \
                ledger_close_time TEXT NOT NULL\
            )",
        )
        .execute(&pool)
        .await
        .expect("create ledger_fee_samples");
        Arc::new(FeeStore::new(pool))
    }

    fn collector_with(store: Arc<FeeStore>, policy: FeeWritePolicy) -> Arc<FeeCollector> {
        Arc::new(
            FeeCollector::new(
                test_registry(),
                store,
                FeeCollectorConfig::default(),
                Arc::new(AppMetrics::new().expect("metrics")),
                test_leader_lock(),
            )
            .with_write_policy(policy)
            .with_alarm_config(SysAlarmConfig::default()),
        )
    }

    fn fast_policy() -> FeeWritePolicy {
        FeeWritePolicy {
            initial_backoff: Duration::from_millis(0),
            max_backoff: Duration::from_millis(0),
            ..FeeWritePolicy::default()
        }
    }

    #[tokio::test]
    async fn retry_with_backoff_succeeds_after_transient_failures() {
        let policy = fast_policy();
        let mut calls = 0usize;
        let outcome = retry_with_backoff(&policy, || {
            calls += 1;
            let attempt = calls;
            async move {
                if attempt < 3 {
                    Err(format!("transient failure {attempt}"))
                } else {
                    Ok(())
                }
            }
        })
        .await;

        assert_eq!(outcome, Ok(3));
        assert_eq!(calls, 3);
    }

    #[tokio::test]
    async fn retry_with_backoff_exhausts_and_reports_last_error() {
        let policy = fast_policy();
        let mut calls = 0usize;
        let outcome = retry_with_backoff(&policy, || {
            calls += 1;
            async move { Err::<(), String>("db unavailable".to_string()) }
        })
        .await;

        assert_eq!(
            outcome,
            Err((policy.max_attempts, "db unavailable".to_string()))
        );
        assert_eq!(calls, policy.max_attempts);
    }

    #[tokio::test]
    async fn exhausted_write_is_dead_lettered_and_not_swallowed() {
        let policy = FeeWritePolicy {
            max_attempts: 2,
            dead_letter_capacity: 8,
            alarm_after_consecutive_failures: 100,
            ..fast_policy()
        };
        let collector = collector_with(failing_store().await, policy.clone());

        let result = collector.persist_sample_with_retry(&sample(42)).await;

        assert!(matches!(result, Err(FeeCollectorError::StoreError(_))));
        assert_eq!(collector.dead_letter_len(), 1);
        let buffered = collector.dead_letter_snapshot();
        assert_eq!(buffered.len(), 1);
        assert_eq!(buffered[0].sample.ledger_sequence, 42);
        assert_eq!(buffered[0].attempts, policy.max_attempts);
        assert!(!buffered[0].last_error.is_empty());
        assert_eq!(collector.consecutive_store_failures(), 1);
        assert_eq!(collector.alarms_raised(), 0);
    }

    #[tokio::test]
    async fn dead_letter_buffer_is_bounded_and_drops_oldest() {
        let policy = FeeWritePolicy {
            max_attempts: 1,
            dead_letter_capacity: 2,
            alarm_after_consecutive_failures: 0,
            ..fast_policy()
        };
        let collector = collector_with(failing_store().await, policy);

        for ledger in [1, 2, 3] {
            let _ = collector.persist_sample_with_retry(&sample(ledger)).await;
        }

        assert_eq!(collector.dead_letter_len(), 2);
        assert_eq!(collector.dead_letters_dropped(), 1);
        let buffered = collector.dead_letter_snapshot();
        assert_eq!(buffered[0].sample.ledger_sequence, 2);
        assert_eq!(buffered[1].sample.ledger_sequence, 3);
    }

    #[tokio::test]
    async fn drain_dead_letters_returns_records_and_empties_buffer() {
        let policy = FeeWritePolicy {
            max_attempts: 1,
            alarm_after_consecutive_failures: 0,
            ..fast_policy()
        };
        let collector = collector_with(failing_store().await, policy);

        for ledger in [10, 11] {
            let _ = collector.persist_sample_with_retry(&sample(ledger)).await;
        }
        assert_eq!(collector.dead_letter_len(), 2);

        let drained = collector.drain_dead_letters();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].sample.ledger_sequence, 10);
        assert_eq!(drained[1].sample.ledger_sequence, 11);
        assert_eq!(collector.dead_letter_len(), 0);
    }

    #[tokio::test]
    async fn alarm_is_raised_at_consecutive_failure_threshold() {
        let policy = FeeWritePolicy {
            max_attempts: 1,
            alarm_after_consecutive_failures: 3,
            ..fast_policy()
        };
        let collector = collector_with(failing_store().await, policy);

        for ledger in 1..=2 {
            let _ = collector.persist_sample_with_retry(&sample(ledger)).await;
        }
        assert_eq!(collector.alarms_raised(), 0);
        assert_eq!(collector.consecutive_store_failures(), 2);

        let _ = collector.persist_sample_with_retry(&sample(3)).await;
        assert_eq!(collector.alarms_raised(), 1);

        let _ = collector.persist_sample_with_retry(&sample(4)).await;
        assert_eq!(collector.alarms_raised(), 1);

        let _ = collector.persist_sample_with_retry(&sample(5)).await;
        assert_eq!(collector.alarms_raised(), 1);

        let _ = collector.persist_sample_with_retry(&sample(6)).await;
        assert_eq!(collector.alarms_raised(), 2);
    }

    #[tokio::test]
    async fn successful_write_does_not_dead_letter_or_alarm() {
        let collector = collector_with(writable_store().await, fast_policy());

        assert!(collector
            .persist_sample_with_retry(&sample(99))
            .await
            .is_ok());
        assert_eq!(collector.dead_letter_len(), 0);
        assert_eq!(collector.consecutive_store_failures(), 0);
        assert_eq!(collector.alarms_raised(), 0);
    }
}
