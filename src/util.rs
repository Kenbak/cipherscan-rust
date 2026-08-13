//! Shared utilities for the CipherScan indexer.

use std::time::{SystemTime, UNIX_EPOCH};

/// Convert a 32-byte Zcash internal hash (block hash, txid) to its display
/// form: reverse byte order and hex-encode. Zcash stores hashes in
/// little-endian internally but displays them in big-endian (like Bitcoin).
pub fn display_hash(bytes: &[u8; 32]) -> String {
    let mut rev = *bytes;
    rev.reverse();
    hex::encode(rev)
}

/// Current Unix timestamp in seconds (monotonic-safe fallback to 0).
pub fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Parse an optional String to u32.
pub fn parse_optional_u32(value: Option<String>) -> Option<u32> {
    value.and_then(|v| v.parse().ok())
}

/// Parse an optional String to u64.
pub fn parse_optional_u64(value: Option<String>) -> Option<u64> {
    value.and_then(|v| v.parse().ok())
}
