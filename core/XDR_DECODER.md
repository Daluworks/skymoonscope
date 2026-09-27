# XDR transaction-result decoder

`XdrTransactionResultDecoder` converts Soroban RPC's base64 XDR fields into
serializable, human-readable diagnostics.

- `decode_result_meta(result_meta_xdr)` decodes the `resultMetaXdr` returned by
  `getTransaction`.
- `decode_soroban_meta_xdr(meta_xdr)` decodes standalone `SorobanTransactionMeta`.
- `decode_envelope(envelope_xdr)` extracts each `InvokeHostFunction` contract
  invocation from `envelopeXdr`.
- `decode(result_meta_xdr, Some(envelope_xdr))` combines both sources.

The decoded result includes contract events, diagnostic events, return value,
and non-refundable, refundable, and rent resource fees. The transaction source
is preserved as the XDR debug representation so muxed accounts remain lossless.

## Error handling and panic safety

Every entry point returns `Result<_, XdrDecodeError>` and never panics on
attacker-controlled input. Malformed input is surfaced through:

- `XdrDecodeError::InvalidBase64` — the payload is not valid base64.
- `XdrDecodeError::InvalidXdr` — the bytes are valid base64 but not valid XDR
  for the requested type (with the byte offset reached by the reader).
- `XdrDecodeError::MissingSorobanMetadata` — `resultMetaXdr` carries no Soroban
  execution metadata.
- `XdrDecodeError::CorruptPayload` — the XDR is structurally valid but encodes
  values that cannot be represented (for example a resource-fee total that
  overflows `i64`).

`XdrDecodeError::to_error_info()` returns a serializable summary and
`offset()` exposes the byte offset when one is known. The `xdr_decode` fuzz
target (`core/fuzz/fuzz_targets/xdr_decode.rs`) drives every entry point with
arbitrary bytes and asserts they never panic.
