//! Fuzz target: feed arbitrary bytes to every `XdrTransactionResultDecoder`
//! entry point and assert the decoder never panics.
//!
//! Run with:
//!   cargo fuzz run xdr_decode
//!
//! The first byte selects which entry point is exercised; the remainder is
//! fed both raw (as text) and base64-encoded, so the fuzzer covers the
//! base64 error path as well as the XDR reader. Malformed, truncated and
//! oversized payloads must surface as `Err`, never as a panic.
#![no_main]

use base64::prelude::BASE64_STANDARD as BASE64;
use base64::Engine;
use libfuzzer_sys::fuzz_target;
use sky_moon_scope_core::xdr_decoder::XdrTransactionResultDecoder;

fuzz_target!(|data: &[u8]| {
    let (selector, payload) = match data.split_first() {
        Some((selector, payload)) => (*selector, payload),
        None => return,
    };

    // Same bytes in both shapes the RPC can hand us: raw text and base64.
    let text = String::from_utf8_lossy(payload).into_owned();
    let encoded = BASE64.encode(payload);

    for input in [&text, &encoded] {
        match selector % 4 {
            0 => {
                let _ = XdrTransactionResultDecoder::decode_result_meta(input);
            }
            1 => {
                let _ = XdrTransactionResultDecoder::decode_soroban_meta_xdr(input);
            }
            2 => {
                let _ = XdrTransactionResultDecoder::decode_envelope(input);
            }
            _ => {
                let _ = XdrTransactionResultDecoder::decode(input, Some(input));
            }
        }
    }

    // Cross-feed: decode() with a mismatched result/envelope pair must also
    // stay panic-free.
    let _ = XdrTransactionResultDecoder::decode(&text, Some(&encoded));
    let _ = XdrTransactionResultDecoder::decode(&encoded, Some(&text));
});
