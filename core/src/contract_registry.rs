//! Whitelisted contract bytecode verification for the event ingestion pipeline.
//!
//! Events emitted by malicious proxy contracts masquerading as a verified
//! instance could corrupt the telemetry index. Before an event is indexed,
//! its `contract_id` is checked against a whitelist of expected WASM
//! bytecode hashes fetched live from the ledger, so a proxy that reuses a
//! whitelisted address (or event shape) but not its bytecode is rejected.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

/// A single whitelist entry: a known-good contract and the SHA-256 hash of
/// its deployed WASM bytecode (lowercase hex).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ContractRegisterEntry {
    pub contract_id: String,
    pub wasm_hash: String,
}

/// Whitelist of `contract_id -> expected WASM bytecode hash` (lowercase hex).
#[derive(Debug, Clone, Default)]
pub struct ContractRegister {
    entries: HashMap<String, String>,
}

impl ContractRegister {
    pub fn new(entries: HashMap<String, String>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(id, hash)| (id, hash.to_lowercase()))
                .collect(),
        }
    }

    /// Register a single contract in the whitelist, rejecting a duplicate
    /// `contract_id` with a [`ContractRegistryError::Conflict`] instead of
    /// silently overwriting the prior entry.
    ///
    /// This is the in-memory analogue of `INSERT ... ON CONFLICT DO NOTHING
    /// RETURNING id`: registration is idempotent-safe for new ids and reports
    /// an explicit conflict when the same contract id is registered twice.
    pub fn register(
        &mut self,
        contract_id: impl Into<String>,
        wasm_hash: impl Into<String>,
    ) -> Result<(), ContractRegistryError> {
        let contract_id = contract_id.into();

        if self.entries.contains_key(&contract_id) {
            return Err(ContractRegistryError::Conflict {
                contract_id: contract_id.clone(),
                existing_hash: self.entries.get(&contract_id).cloned().unwrap_or_default(),
            });
        }
        self.entries
            .insert(contract_id, wasm_hash.into().to_lowercase());
        Ok(())
    }

    /// Parse a whitelist from a JSON array of `{"contract_id", "wasm_hash"}`
    /// entries, e.g. the `VERIFIED_CONTRACTS` environment variable. An empty
    /// or absent value yields an empty (deny-all) register.
    ///
    /// A `contract_id` appearing more than once is rejected with a
    /// [`ContractRegistryError::Conflict`] rather than silently keeping the
    /// last entry.
    pub fn from_json(json: &str) -> Result<Self, ContractRegistryError> {
        let trimmed = json.trim();
        if trimmed.is_empty() {
            return Ok(Self::default());
        }

        let entries: Vec<ContractRegisterEntry> = serde_json::from_str(trimmed)
            .map_err(|e| ContractRegistryError::InvalidWhitelist(e.to_string()))?;

        let mut register = Self::default();
        for entry in entries {
            register.register(entry.contract_id, entry.wasm_hash)?;
        }
        Ok(register)
    }

    pub fn expected_hash(&self, contract_id: &str) -> Option<&str> {
        self.entries.get(contract_id).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Error)]
pub enum ContractRegistryError {
    #[error("invalid contract whitelist configuration: {0}")]
    InvalidWhitelist(String),
    #[error("contract {0} is not present in the verified contract register")]
    NotWhitelisted(String),
    #[error(
        "bytecode hash mismatch for contract {contract_id}: expected {expected}, found {actual} \
         — possible spoofed proxy contract"
    )]
    HashMismatch {
        contract_id: String,
        expected: String,
        actual: String,
    },
    /// The contract id was already present in the register when a duplicate
    /// registration was attempted. HTTP callers should surface this as
    /// `409 Conflict`.
    #[error("conflict: contract {contract_id} is already registered with hash {existing_hash}")]
    Conflict {
        contract_id: String,
        existing_hash: String,
    },
    #[error("failed to fetch contract bytecode: {0}")]
    Source(String),
}

/// Fetches a contract's live WASM bytecode so it can be hashed and compared
/// against the whitelist. Implemented for [`crate::simulation::SimulationEngine`]
/// in production; tests inject a fake to avoid live RPC calls.
pub trait ContractBytecodeSource {
    fn fetch_wasm(
        &self,
        contract_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, String>> + Send;
}

impl ContractBytecodeSource for crate::simulation::SimulationEngine {
    async fn fetch_wasm(&self, contract_id: &str) -> Result<Vec<u8>, String> {
        self.get_contract_wasm(contract_id)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Verify that `contract_id`'s on-ledger WASM bytecode hash matches the
/// whitelist before the caller is allowed to index/dispatch its events.
pub async fn verify_bytecode<S: ContractBytecodeSource>(
    source: &S,
    registry: &ContractRegister,
    contract_id: &str,
) -> Result<(), ContractRegistryError> {
    let expected = registry
        .expected_hash(contract_id)
        .ok_or_else(|| ContractRegistryError::NotWhitelisted(contract_id.to_string()))?
        .to_string();

    let wasm_bytes = source
        .fetch_wasm(contract_id)
        .await
        .map_err(ContractRegistryError::Source)?;
    let actual = hex::encode(Sha256::digest(&wasm_bytes));

    if actual == expected {
        Ok(())
    } else {
        Err(ContractRegistryError::HashMismatch {
            contract_id: contract_id.to_string(),
            expected,
            actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource {
        wasm_by_contract: HashMap<String, Vec<u8>>,
    }

    impl ContractBytecodeSource for FakeSource {
        async fn fetch_wasm(&self, contract_id: &str) -> Result<Vec<u8>, String> {
            self.wasm_by_contract
                .get(contract_id)
                .cloned()
                .ok_or_else(|| format!("no wasm registered for {contract_id}"))
        }
    }

    fn hash_hex(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    #[tokio::test]
    async fn accepts_contract_whose_live_bytecode_matches_the_whitelist() {
        let wasm = b"legit-contract-wasm".to_vec();
        let source = FakeSource {
            wasm_by_contract: HashMap::from([("CGOOD".to_string(), wasm.clone())]),
        };
        let registry =
            ContractRegister::new(HashMap::from([("CGOOD".to_string(), hash_hex(&wasm))]));

        assert!(verify_bytecode(&source, &registry, "CGOOD").await.is_ok());
    }

    #[tokio::test]
    async fn rejects_contract_not_present_in_the_whitelist() {
        let source = FakeSource {
            wasm_by_contract: HashMap::new(),
        };
        let registry = ContractRegister::default();

        let err = verify_bytecode(&source, &registry, "CUNKNOWN")
            .await
            .unwrap_err();
        assert!(matches!(err, ContractRegistryError::NotWhitelisted(id) if id == "CUNKNOWN"));
    }

    #[tokio::test]
    async fn rejects_spoofed_proxy_contract_whose_bytecode_does_not_match() {
        let real_wasm = b"legit-contract-wasm".to_vec();
        let proxy_wasm = b"malicious-proxy-wasm".to_vec();
        let source = FakeSource {
            wasm_by_contract: HashMap::from([("CSPOOFED".to_string(), proxy_wasm)]),
        };
        let registry = ContractRegister::new(HashMap::from([(
            "CSPOOFED".to_string(),
            hash_hex(&real_wasm),
        )]));

        let err = verify_bytecode(&source, &registry, "CSPOOFED")
            .await
            .unwrap_err();
        assert!(matches!(err, ContractRegistryError::HashMismatch { .. }));
    }

    #[test]
    fn whitelist_hash_lookup_is_case_insensitive() {
        let registry = ContractRegister::new(HashMap::from([(
            "CMIXED".to_string(),
            "AABBCC".to_string(),
        )]));
        assert_eq!(registry.expected_hash("CMIXED"), Some("aabbcc"));
    }

    #[test]
    fn from_json_parses_a_whitelist_array() {
        let json = r#"[{"contract_id":"CABC","wasm_hash":"deadbeef"}]"#;
        let registry = ContractRegister::from_json(json).expect("valid whitelist json");
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.expected_hash("CABC"), Some("deadbeef"));
    }

    #[test]
    fn from_json_empty_string_yields_empty_register() {
        let registry = ContractRegister::from_json("").expect("empty whitelist is valid");
        assert!(registry.is_empty());
    }

    #[test]
    fn from_json_rejects_malformed_input() {
        assert!(ContractRegister::from_json("not json").is_err());
    }

    #[test]
    fn from_json_rejects_duplicate_contract_ids_with_a_conflict() {
        let json = r#"[
            {"contract_id":"CDUP","wasm_hash":"deadbeef"},
            {"contract_id":"CDUP","wasm_hash":"cafebabe"}
        ]"#;
        let err = ContractRegister::from_json(json).unwrap_err();
        assert!(
            matches!(
                &err,
                ContractRegistryError::Conflict {
                    contract_id,
                    existing_hash
                } if contract_id == "CDUP" && existing_hash == "deadbeef"
            ),
            "expected a conflict naming the duplicate contract id, got {err}"
        );
    }

    #[test]
    fn from_json_keeps_distinct_contract_ids() {
        let json = r#"[
            {"contract_id":"CA","wasm_hash":"deadbeef"},
            {"contract_id":"CB","wasm_hash":"cafebabe"}
        ]"#;
        let registry = ContractRegister::from_json(json).expect("distinct ids are valid");
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.expected_hash("CA"), Some("deadbeef"));
        assert_eq!(registry.expected_hash("CB"), Some("cafebabe"));
    }

    #[test]
    fn register_rejects_a_duplicate_contract_id_with_a_conflict() {
        let mut registry =
            ContractRegister::new(HashMap::from([("C1".to_string(), "deadbeef".to_string())]));

        assert!(registry.register("C1", "cafebabe").is_err());
        // The original entry must not have been overwritten.
        assert_eq!(registry.expected_hash("C1"), Some("deadbeef"));
    }

    #[test]
    fn register_accepts_a_new_contract_and_lowercases_its_hash() {
        let mut registry = ContractRegister::default();
        registry
            .register("C2", "CAFEBABE")
            .expect("new contract id registers");
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.expected_hash("C2"), Some("cafebabe"));
    }
}
