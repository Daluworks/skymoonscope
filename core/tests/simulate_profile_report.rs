//! End-to-end test for the WASM upload → simulation → resource profile
//! pipeline (issue #026).
//!
//! Unlike the unit tests in `src/simulation_service.rs`, which hand
//! `record_and_analyze` a hand-built metric, this test drives the whole path:
//!
//! 1. load the `hello_soroban` contract WASM (the artifact produced by
//!    `cargo build -p hello_soroban --target wasm32-unknown-unknown --release`),
//! 2. "upload" it by registering it as a contract in a Soroban host,
//! 3. simulate a call to its `hello` entry point and read the CPU instruction
//!    and memory cost the host charged for upload + execution,
//! 4. store that measurement through `SimulationService`, which is what turns
//!    raw simulation numbers into a resource profile,
//! 5. assert the profile and the generated report: the metric is non-zero and
//!    persisted, and a second measurement on the same code hash produces the
//!    historical baseline the drift analysis is built on.
//!
//! The WASM artifact is looked up, never built, from this test: a test that
//! shells out to cargo would fight the test harness for the target directory.
//! When the artifact is absent the test reports why and returns, which keeps
//! `cargo test` usable in a checkout that only compiles the crate. Build the
//! artifact (command above, or point `HELLO_SOROBAN_WASM` at it) to exercise
//! the real path.

use sky_moon_scope_core::simulation_service::{SimulationMetric, SimulationService};

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use soroban_sdk::{symbol_short, Env, IntoVal, Symbol, Val, Vec};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Environment override for the WASM artifact location.
const WASM_ENV: &str = "HELLO_SOROBAN_WASM";
/// Name of the artifact, shared by every cargo profile.
const WASM_FILE: &str = "hello_soroban.wasm";

/// Same contract/method pair the profile is keyed by. Kept identical between
/// the two measurements so the second one is compared against the first.
const CONTRACT: &str = "hello_soroban";
const METHOD: &str = "hello";

/// Removes the temporary SQLite database when the test finishes, even if an
/// assertion fails.
struct TempDb(PathBuf);

impl TempDb {
    fn new(test_name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sky_moon_scope_{test_name}_{nanos}.db"));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Locates a previously built `hello_soroban.wasm`.
///
/// Candidates are checked in order of likelihood: the explicit override first,
/// then the shared `target/` directory at the repository root (where
/// `cargo build -p hello_soroban --target wasm32-unknown-unknown` puts it when
/// run from the root workspace), then `core/`'s own target directory (this
/// crate is a separate workspace root), then the contract directory.
fn find_hello_soroban_wasm() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var(WASM_ENV) {
        let explicit = PathBuf::from(explicit);
        if explicit.is_file() {
            return Some(explicit);
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap_or(&manifest_dir);

    let candidates = [
        repo_root
            .join("target/wasm32-unknown-unknown/release")
            .join(WASM_FILE),
        repo_root
            .join("target/wasm32-unknown-unknown/debug")
            .join(WASM_FILE),
        manifest_dir
            .join("target/wasm32-unknown-unknown/release")
            .join(WASM_FILE),
        manifest_dir
            .join("target/wasm32-unknown-unknown/debug")
            .join(WASM_FILE),
        manifest_dir
            .join("../contracts/hello_soroban/target/wasm32-unknown-unknown/release")
            .join(WASM_FILE),
        PathBuf::from("target/wasm32-unknown-unknown/release").join(WASM_FILE),
    ];

    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// Simulates a WASM upload plus one `hello` invocation and returns the
/// resource profile for it.
///
/// The host charges the budget for both the upload (`Env::register` compiles
/// and instantiates the module) and the call, which is exactly the work the
/// service is meant to profile. A failing call panics inside
/// `invoke_contract`, so a contract that reverts fails the test rather than
/// silently reporting a tiny cost.
fn simulate_hello(wasm: &[u8]) -> SimulationMetric {
    let env = Env::default();
    env.mock_all_auths();

    let budget_start_cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let budget_start_mem = env.cost_estimate().budget().memory_bytes_cost();

    // Step 2: the "upload" — registering the WASM instantiates it in the host.
    let contract_id = env.register(wasm, ());

    // Step 3: simulate the entry point. `hello` takes one `Symbol` and returns
    // a `Vec<Symbol>`; the host bills the CPU and memory it consumed.
    let to = symbol_short!("Wasm");
    let args: Vec<Val> = Vec::from_array(&env, [to.to_val()]);
    let _result: Val = env.invoke_contract(&contract_id, &Symbol::new(&env, METHOD), args);

    let budget_end_cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let budget_end_mem = env.cost_estimate().budget().memory_bytes_cost();

    let cpu_instructions = budget_end_cpu.saturating_sub(budget_start_cpu);
    let ram_bytes = budget_end_mem.saturating_sub(budget_start_mem);

    SimulationMetric {
        contract: CONTRACT.to_string(),
        method: METHOD.to_string(),
        code_hash: format!("{:x}", Sha256::digest(wasm)),
        cpu_instructions,
        ram_bytes,
        // The host reports memory usage as a byte cost, which is the closest
        // proxy the simulation layer has for ledger footprint.
        ledger_footprint: ram_bytes,
    }
}

/// Sums the persisted profile columns so the assertions below read from the
/// database rather than from the values the test already held in memory.
fn stored_totals(db_path: &Path) -> (i64, i64, i64) {
    let conn = Connection::open(db_path).expect("open the metrics database");
    conn.query_row(
        "SELECT COALESCE(SUM(cpu_instructions), 0), COALESCE(SUM(ram_bytes), 0), COUNT(*)
         FROM simulation_metrics",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .expect("sum the persisted simulation metrics")
}

#[tokio::test]
async fn wasm_upload_simulation_and_resource_profile_end_to_end() {
    let Some(wasm_path) = find_hello_soroban_wasm() else {
        eprintln!(
            "skipping wasm_upload_simulation_and_resource_profile_end_to_end: no {WASM_FILE} found.\n\
             Build it with:\n    \
             cargo build -p hello_soroban --target wasm32-unknown-unknown --release\n\
             or set {WASM_ENV} to the artifact path."
        );
        return;
    };

    let wasm = std::fs::read(&wasm_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", wasm_path.display()));
    assert!(!wasm.is_empty(), "{} is empty", wasm_path.display());

    // ── Upload + simulate twice, so the second call exercises the profile
    //    branch (a baseline already exists for this code hash).
    let first_metric = simulate_hello(&wasm);
    let second_metric = simulate_hello(&wasm);

    assert_eq!(
        first_metric.code_hash, second_metric.code_hash,
        "the same artifact must hash identically"
    );

    // ── Step 4: turn the simulation into a stored resource profile.
    let db = TempDb::new("simulate_profile_report");
    let service = SimulationService::new(db.path(), None)
        .expect("initialise the simulation service on a fresh database");

    let first_report = service
        .record_and_analyze(first_metric.clone())
        .await
        .expect("the first measurement should be stored and analysed");
    assert!(
        !first_report.has_historical_baseline,
        "the first measurement for a code hash has nothing to compare against"
    );
    assert!(!first_report.alert_triggered);
    assert!(first_report.outliers.is_empty());

    let second_report = service
        .record_and_analyze(second_metric.clone())
        .await
        .expect("the second measurement should be stored and analysed");

    // ── Step 5: assert the generated profile and report.
    assert!(
        first_metric.cpu_instructions > 0,
        "the host must bill CPU instructions for upload + execution, got {}",
        first_metric.cpu_instructions
    );
    assert!(
        first_metric.ram_bytes > 0,
        "the host must bill memory for upload + execution, got {}",
        first_metric.ram_bytes
    );
    assert!(
        first_metric.ledger_footprint > 0,
        "the simulated ledger footprint must be non-zero"
    );

    assert!(
        second_report.has_historical_baseline,
        "the second measurement should be compared against the first"
    );
    let historical = second_report
        .historical
        .expect("a baseline report must carry the historical averages");
    assert_eq!(historical.samples, 1);
    assert_eq!(
        historical.avg_cpu_instructions, first_metric.cpu_instructions as f64,
        "the profile must record the CPU cost of the first simulation"
    );
    assert_eq!(
        historical.avg_ram_bytes, first_metric.ram_bytes as f64,
        "the profile must record the RAM cost of the first simulation"
    );
    assert_eq!(
        historical.avg_ledger_footprint, first_metric.ledger_footprint as f64
    );

    // Running the same artifact again is not a code change, so the drift
    // analysis must stay quiet instead of paging on simulation noise.
    assert!(
        !second_report.alert_triggered,
        "an unchanged code hash must not raise a resource-shift alert: {:?}",
        second_report.outliers
    );

    // ...and the metrics really landed in the profile table, with non-zero
    // CPU/RAM for both simulations.
    let (cpu_total, ram_total, rows) = stored_totals(db.path());
    assert_eq!(rows, 2, "both simulations should be persisted");
    assert!(cpu_total > 0, "persisted CPU instructions must be non-zero");
    assert!(ram_total > 0, "persisted RAM bytes must be non-zero");
    assert_eq!(
        cpu_total,
        (first_metric.cpu_instructions + second_metric.cpu_instructions) as i64
    );
}
