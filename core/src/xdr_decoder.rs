//! Decodes Soroban transaction-result XDR into API-friendly diagnostics.

use base64::prelude::BASE64_STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use soroban_sdk::xdr::{
    ContractEvent, ContractEventBody, HostFunction, Limits, Operation, OperationBody, ReadXdr,
    SorobanTransactionMeta, SorobanTransactionMetaExt, TransactionEnvelope, TransactionMeta,
    TransactionResultMeta,
};
use std::io::Cursor;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecodedTransactionResult {
    pub invocations: Vec<DecodedInvocation>,
    pub events: Vec<DecodedContractEvent>,
    pub diagnostics: Vec<DecodedDiagnosticEvent>,
    pub return_value: String,
    pub fees: ResourceFeeBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecodedInvocation {
    /// Transaction source account in lossless XDR debug representation.
    pub invoker: String,
    pub contract: String,
    pub function: String,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecodedContractEvent {
    pub contract_id: Option<String>,
    pub event_type: String,
    pub topics: Vec<String>,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecodedDiagnosticEvent {
    pub in_successful_contract_call: bool,
    pub event: DecodedContractEvent,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceFeeBreakdown {
    pub non_refundable: i64,
    pub refundable: i64,
    pub rent: i64,
    pub total: i64,
}

/// Detailed error information containing the failure kind, byte offset, and message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Error)]
#[error("invalid {kind} XDR{}: {message}", match .offset { Some(off) => format!(" at byte offset {off}"), None => String::new() })]
pub struct XdrDecodeErrorInfo {
    pub kind: String,
    pub offset: Option<usize>,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum XdrDecodeError {
    #[error("invalid {kind} XDR{}: {message}", match .offset { Some(off) => format!(" at byte offset {off}"), None => String::new() })]
    InvalidXdr {
        kind: &'static str,
        offset: Option<usize>,
        message: String,
        #[source]
        source: Option<soroban_sdk::xdr::Error>,
    },
    #[error("invalid base64 encoding{}: {message}", match .offset { Some(off) => format!(" at byte offset {off}"), None => String::new() })]
    InvalidBase64 {
        offset: Option<usize>,
        message: String,
    },
    #[error("corrupted XDR payload: {0}")]
    CorruptPayload(XdrDecodeErrorInfo),
    #[error("transaction metadata does not contain Soroban execution data")]
    MissingSorobanMetadata,
}

impl XdrDecodeError {
    pub fn offset(&self) -> Option<usize> {
        match self {
            XdrDecodeError::InvalidXdr { offset, .. } => *offset,
            XdrDecodeError::InvalidBase64 { offset, .. } => *offset,
            XdrDecodeError::CorruptPayload(info) => info.offset,
            XdrDecodeError::MissingSorobanMetadata => None,
        }
    }

    pub fn to_error_info(&self) -> XdrDecodeErrorInfo {
        match self {
            XdrDecodeError::InvalidXdr {
                kind,
                offset,
                message,
                ..
            } => XdrDecodeErrorInfo {
                kind: kind.to_string(),
                offset: *offset,
                message: message.clone(),
            },
            XdrDecodeError::InvalidBase64 { offset, message } => XdrDecodeErrorInfo {
                kind: "base64".to_string(),
                offset: *offset,
                message: message.clone(),
            },
            XdrDecodeError::CorruptPayload(info) => info.clone(),
            XdrDecodeError::MissingSorobanMetadata => XdrDecodeErrorInfo {
                kind: "soroban_metadata".to_string(),
                offset: None,
                message: "transaction metadata does not contain Soroban execution data".to_string(),
            },
        }
    }
}

pub struct XdrTransactionResultDecoder;

impl XdrTransactionResultDecoder {
    /// Helper to decode base64 into raw bytes and then parse XDR with byte offset tracking.
    fn decode_xdr_base64<T: ReadXdr>(xdr: &str, kind: &'static str) -> Result<T, XdrDecodeError> {
        let trimmed = xdr.trim();
        let bytes = match BASE64.decode(trimmed.as_bytes()) {
            Ok(b) => b,
            Err(e) => {
                let offset = match e {
                    base64::DecodeError::InvalidByte(offset, _) => Some(offset),
                    base64::DecodeError::InvalidLength(len) => Some(len),
                    base64::DecodeError::InvalidLastSymbol(offset, _) => Some(offset),
                    base64::DecodeError::InvalidPadding => None,
                };
                return Err(XdrDecodeError::InvalidBase64 {
                    offset,
                    message: format!("base64 decode error: {e}"),
                });
            }
        };

        let mut cursor = Cursor::new(&bytes);
        match T::read_xdr(&mut cursor, Limits::none()) {
            Ok(val) => Ok(val),
            Err(source) => {
                let offset = Some(cursor.position() as usize);
                Err(XdrDecodeError::InvalidXdr {
                    kind,
                    offset,
                    message: format!("{source}"),
                    source: Some(source),
                })
            }
        }
    }

    /// Decodes the `resultMetaXdr` returned by Soroban RPC `getTransaction`.
    pub fn decode_result_meta(xdr: &str) -> Result<DecodedTransactionResult, XdrDecodeError> {
        let result: TransactionResultMeta =
            Self::decode_xdr_base64(xdr, "transaction result metadata")?;
        let meta = soroban_meta(&result.tx_apply_processing)
            .ok_or(XdrDecodeError::MissingSorobanMetadata)?;
        Self::decode_soroban_meta(meta)
    }

    /// Decodes standalone `SorobanTransactionMeta` XDR.
    pub fn decode_soroban_meta_xdr(xdr: &str) -> Result<DecodedTransactionResult, XdrDecodeError> {
        let meta: SorobanTransactionMeta =
            Self::decode_xdr_base64(xdr, "Soroban transaction metadata")?;
        Self::decode_soroban_meta(&meta)
    }

    /// Decodes host-function invocations from an RPC `envelopeXdr` value.
    pub fn decode_envelope(xdr: &str) -> Result<Vec<DecodedInvocation>, XdrDecodeError> {
        let envelope: TransactionEnvelope =
            Self::decode_xdr_base64(xdr, "transaction envelope")?;
        Ok(decode_envelope(&envelope))
    }

    /// Decodes a result and optionally enriches it with invocation details.
    pub fn decode(
        result_xdr: &str,
        envelope_xdr: Option<&str>,
    ) -> Result<DecodedTransactionResult, XdrDecodeError> {
        let mut decoded = Self::decode_result_meta(result_xdr)?;
        if let Some(xdr) = envelope_xdr {
            decoded.invocations = Self::decode_envelope(xdr)?;
        }
        Ok(decoded)
    }

    /// Decodes a `SorobanTransactionMeta` into API-friendly diagnostics.
    ///
    /// This is fallible because field values are attacker-controlled: a
    /// malformed payload can encode resource fees whose sum overflows `i64`.
    /// Such inputs return `Err` instead of panicking.
    pub fn decode_soroban_meta(
        meta: &SorobanTransactionMeta,
    ) -> Result<DecodedTransactionResult, XdrDecodeError> {
        let fees = match &meta.ext {
            SorobanTransactionMetaExt::V0 => ResourceFeeBreakdown::default(),
            SorobanTransactionMetaExt::V1(fees) => {
                let total = fees
                    .total_non_refundable_resource_fee_charged
                    .checked_add(fees.total_refundable_resource_fee_charged)
                    .ok_or_else(|| {
                        XdrDecodeError::CorruptPayload(XdrDecodeErrorInfo {
                            kind: "soroban_metadata".to_string(),
                            offset: None,
                            message: "resource fee total overflows i64".to_string(),
                        })
                    })?;
                ResourceFeeBreakdown {
                    non_refundable: fees.total_non_refundable_resource_fee_charged,
                    refundable: fees.total_refundable_resource_fee_charged,
                    rent: fees.rent_fee_charged,
                    total,
                }
            }
        };
        Ok(DecodedTransactionResult {
            invocations: Vec::new(),
            events: meta.events.iter().map(decode_event).collect(),
            diagnostics: meta
                .diagnostic_events
                .iter()
                .map(|diagnostic| DecodedDiagnosticEvent {
                    in_successful_contract_call: diagnostic.in_successful_contract_call,
                    event: decode_event(&diagnostic.event),
                })
                .collect(),
            return_value: format_sc_val(&meta.return_value),
            fees,
        })
    }
}

fn soroban_meta(meta: &TransactionMeta) -> Option<&SorobanTransactionMeta> {
    match meta {
        TransactionMeta::V3(meta) => meta.soroban_meta.as_ref(),
        TransactionMeta::V0(_) | TransactionMeta::V1(_) | TransactionMeta::V2(_) => None,
    }
}

fn decode_envelope(envelope: &TransactionEnvelope) -> Vec<DecodedInvocation> {
    match envelope {
        TransactionEnvelope::Tx(envelope) => {
            decode_operations(&envelope.tx.source_account, &envelope.tx.operations)
        }
        TransactionEnvelope::TxFeeBump(envelope) => match &envelope.tx.inner_tx {
            soroban_sdk::xdr::FeeBumpTransactionInnerTx::Tx(inner) => {
                decode_operations(&inner.tx.source_account, &inner.tx.operations)
            }
        },
        TransactionEnvelope::TxV0(_) => Vec::new(),
    }
}

fn decode_operations(
    source: &soroban_sdk::xdr::MuxedAccount,
    operations: &[Operation],
) -> Vec<DecodedInvocation> {
    operations
        .iter()
        .filter_map(|operation| match &operation.body {
            OperationBody::InvokeHostFunction(op) => match &op.host_function {
                HostFunction::InvokeContract(args) => Some(DecodedInvocation {
                    invoker: format!("{source:?}"),
                    contract: format!("{:?}", args.contract_address),
                    function: format_symbol(&args.function_name),
                    arguments: args.args.iter().map(format_sc_val).collect(),
                }),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn decode_event(event: &ContractEvent) -> DecodedContractEvent {
    let (topics, data) = match &event.body {
        ContractEventBody::V0(body) => (
            body.topics.iter().map(format_sc_val).collect(),
            format_sc_val(&body.data),
        ),
    };
    DecodedContractEvent {
        contract_id: event.contract_id.as_ref().map(|id| hex::encode(id.0)),
        event_type: event.type_.to_string(),
        topics,
        data,
    }
}

fn format_symbol(symbol: &soroban_sdk::xdr::ScSymbol) -> String {
    String::from_utf8_lossy(symbol.0.as_ref()).into_owned()
}

fn format_sc_val(value: &soroban_sdk::xdr::ScVal) -> String {
    use soroban_sdk::xdr::ScVal;
    match value {
        ScVal::Void => "void".to_string(),
        ScVal::Bool(value) => value.to_string(),
        ScVal::U32(value) => value.to_string(),
        ScVal::I32(value) => value.to_string(),
        ScVal::U64(value) => value.to_string(),
        ScVal::I64(value) => value.to_string(),
        ScVal::U128(value) => format!("{value:?}"),
        ScVal::I128(value) => format!("{value:?}"),
        ScVal::U256(value) => format!("{value:?}"),
        ScVal::I256(value) => format!("{value:?}"),
        ScVal::Symbol(value) => format!(":{}", format_symbol(value)),
        ScVal::String(value) => String::from_utf8_lossy(value.0.as_ref()).into_owned(),
        ScVal::Bytes(value) => format!("0x{}", hex::encode(value.0.as_slice())),
        _ => format!("{value:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::xdr::{ExtensionPoint, ScVal, SorobanTransactionMetaExtV1, VecM, WriteXdr};

    #[test]
    fn decodes_return_value_and_v0_fees_from_xdr() {
        let meta = SorobanTransactionMeta {
            ext: SorobanTransactionMetaExt::V0,
            events: VecM::default(),
            return_value: ScVal::U32(42),
            diagnostic_events: VecM::default(),
        };
        let xdr = meta.to_xdr_base64(Limits::none()).unwrap();
        let decoded = XdrTransactionResultDecoder::decode_soroban_meta_xdr(&xdr).unwrap();
        assert_eq!(decoded.return_value, "42");
        assert_eq!(decoded.fees, ResourceFeeBreakdown::default());
    }

    #[test]
    fn rejects_invalid_base64_xdr_with_offset() {
        let error = XdrTransactionResultDecoder::decode_soroban_meta_xdr("not xdr").unwrap_err();
        assert!(matches!(error, XdrDecodeError::InvalidBase64 { .. }));
        let info = error.to_error_info();
        assert_eq!(info.kind, "base64");
        assert!(info.offset.is_some());
    }

    #[test]
    fn rejects_corrupt_payload_bytes_with_offset() {
        // Valid base64 encoding of random corrupt bytes (not valid SorobanTransactionMeta)
        let corrupt_base64 = BASE64.encode(&[0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE]);
        let error = XdrTransactionResultDecoder::decode_soroban_meta_xdr(&corrupt_base64).unwrap_err();
        assert!(matches!(error, XdrDecodeError::InvalidXdr { .. }));
        let info = error.to_error_info();
        assert_eq!(info.kind, "Soroban transaction metadata");
        assert!(info.offset.is_some());
    }

    #[test]
    fn rejects_corrupt_envelope_payload() {
        let corrupt_base64 = BASE64.encode(b"malformed_envelope_bytes");
        let error = XdrTransactionResultDecoder::decode_envelope(&corrupt_base64).unwrap_err();
        assert!(matches!(error, XdrDecodeError::InvalidXdr { .. }));
        let info = error.to_error_info();
        assert_eq!(info.kind, "transaction envelope");
        assert!(info.offset.is_some());
    }

    #[test]
    fn rejects_corrupt_result_meta_payload() {
        let corrupt_base64 = BASE64.encode(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let error = XdrTransactionResultDecoder::decode_result_meta(&corrupt_base64).unwrap_err();
        assert!(matches!(error, XdrDecodeError::InvalidXdr { .. }));
        let info = error.to_error_info();
        assert_eq!(info.kind, "transaction result metadata");
        assert!(info.offset.is_some());
    }

    #[test]
    fn handles_empty_or_whitespace_xdr_without_panicking() {
        // Empty and whitespace-only inputs must surface as `Err`, not a panic.
        assert!(XdrTransactionResultDecoder::decode_soroban_meta_xdr("   ").is_err());
        assert!(XdrTransactionResultDecoder::decode_soroban_meta_xdr("").is_err());
        assert!(XdrTransactionResultDecoder::decode_result_meta("").is_err());
        assert!(XdrTransactionResultDecoder::decode_envelope("").is_err());
    }

    #[test]
    fn rejects_truncated_soroban_meta_payload() {
        let meta = SorobanTransactionMeta {
            ext: SorobanTransactionMetaExt::V0,
            events: VecM::default(),
            return_value: ScVal::U32(42),
            diagnostic_events: VecM::default(),
        };
        let full = BASE64
            .decode(meta.to_xdr_base64(Limits::none()).unwrap())
            .unwrap();
        // Drop the tail so the reader hits EOF mid-structure.
        let truncated = BASE64.encode(&full[..full.len() / 2]);
        let error = XdrTransactionResultDecoder::decode_soroban_meta_xdr(&truncated).unwrap_err();
        assert!(matches!(error, XdrDecodeError::InvalidXdr { .. }));
        assert!(error.offset().is_some());
    }

    #[test]
    fn rejects_unknown_meta_ext_discriminant() {
        // A bogus enum discriminant for `SorobanTransactionMetaExt` (i32::MAX).
        let encoded = BASE64.encode([0xFF, 0xFF, 0xFF, 0x7F]);
        let error = XdrTransactionResultDecoder::decode_soroban_meta_xdr(&encoded).unwrap_err();
        assert!(matches!(error, XdrDecodeError::InvalidXdr { .. }));
    }

    #[test]
    fn rejects_fee_total_overflow_instead_of_panicking() {
        let meta = SorobanTransactionMeta {
            ext: SorobanTransactionMetaExt::V1(SorobanTransactionMetaExtV1 {
                ext: ExtensionPoint::V0,
                total_non_refundable_resource_fee_charged: i64::MAX,
                total_refundable_resource_fee_charged: 1,
                rent_fee_charged: 0,
            }),
            events: VecM::default(),
            return_value: ScVal::Void,
            diagnostic_events: VecM::default(),
        };

        // The helper must return Err rather than overflow-panicking.
        let error = XdrTransactionResultDecoder::decode_soroban_meta(&meta).unwrap_err();
        assert!(matches!(error, XdrDecodeError::CorruptPayload(_)));
        assert_eq!(error.offset(), None);

        // ...and the same crafted payload through the string entry point.
        let xdr = meta.to_xdr_base64(Limits::none()).unwrap();
        let error = XdrTransactionResultDecoder::decode_soroban_meta_xdr(&xdr).unwrap_err();
        assert!(matches!(error, XdrDecodeError::CorruptPayload(_)));
    }

    #[test]
    fn decode_propagates_malformed_envelope_error() {
        let meta = SorobanTransactionMeta {
            ext: SorobanTransactionMetaExt::V0,
            events: VecM::default(),
            return_value: ScVal::U32(7),
            diagnostic_events: VecM::default(),
        };
        let xdr = meta.to_xdr_base64(Limits::none()).unwrap();
        let error =
            XdrTransactionResultDecoder::decode(&xdr, Some("!!! not base64 !!!")).unwrap_err();
        assert!(matches!(error, XdrDecodeError::InvalidBase64 { .. }));
    }

    #[test]
    fn all_entry_points_return_err_for_arbitrary_garbage() {
        let garbage = BASE64.encode(b"\x00\x01\x02\x03\x04\x05\x06\x07");
        assert!(XdrTransactionResultDecoder::decode_result_meta(&garbage).is_err());
        assert!(XdrTransactionResultDecoder::decode_soroban_meta_xdr(&garbage).is_err());
        assert!(XdrTransactionResultDecoder::decode_envelope(&garbage).is_err());
        assert!(XdrTransactionResultDecoder::decode(&garbage, Some(&garbage)).is_err());
    }
}
