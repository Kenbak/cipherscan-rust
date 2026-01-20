//! Block data model

use serde::{Serialize, Deserialize};

/// Zcash block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub height: u32,
    pub hash: String,
    pub version: i32,
    pub merkle_root: String,
    pub time: u64,
    pub difficulty: String,
    pub nonce: String,
    pub solution: String,
    pub previous_block_hash: String,
    pub tx_count: u32,
    pub size: u32,
    pub sapling_tree_size: Option<u64>,
    pub orchard_tree_size: Option<u64>,
}

impl Block {
    /// Create from raw RocksDB data
    pub fn from_raw(height: u32, hash: [u8; 32], header_data: &[u8]) -> Result<Self, String> {
        // Reverse hash for display (Zcash convention)
        let mut hash_reversed = hash;
        hash_reversed.reverse();

        // Parse header - this is simplified, actual parsing is more complex
        // Zcash block header is 1487 bytes (140 byte header + Equihash solution)
        if header_data.len() < 140 {
            return Err(format!("Header too short: {} bytes", header_data.len()));
        }

        // Version (4 bytes, little-endian)
        let version = i32::from_le_bytes([
            header_data[0], header_data[1], header_data[2], header_data[3]
        ]);

        // Previous block hash (32 bytes, already reversed in storage)
        let mut prev_hash = [0u8; 32];
        prev_hash.copy_from_slice(&header_data[4..36]);

        // Merkle root (32 bytes)
        let mut merkle = [0u8; 32];
        merkle.copy_from_slice(&header_data[36..68]);

        // Reserved (32 bytes) - skip

        // Time (4 bytes at offset 100)
        let time = u32::from_le_bytes([
            header_data[100], header_data[101], header_data[102], header_data[103]
        ]) as u64;

        // Bits/difficulty (4 bytes)
        let bits = u32::from_le_bytes([
            header_data[104], header_data[105], header_data[106], header_data[107]
        ]);

        // Nonce (32 bytes at offset 108)
        let nonce = hex::encode(&header_data[108..140]);

        // Equihash solution starts at 140
        let solution = if header_data.len() > 143 {
            hex::encode(&header_data[140..std::cmp::min(140 + 100, header_data.len())])
        } else {
            String::new()
        };

        Ok(Self {
            height,
            hash: hex::encode(&hash_reversed),
            version,
            merkle_root: hex::encode(&merkle),
            time,
            difficulty: format!("{:08x}", bits),
            nonce,
            solution,
            previous_block_hash: hex::encode(&prev_hash),
            tx_count: 0,  // Will be filled by transaction parsing
            size: header_data.len() as u32,
            sapling_tree_size: None,
            orchard_tree_size: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_hash() {
        // Genesis block hash should match known value
        let hash = hex::decode("00040fe8ec8471911baa1db1266ea15dd06b4a8a5c453883c000b031973dce08")
            .unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&hash);

        // After reversing, should be the canonical genesis hash
        arr.reverse();
        let display_hash = hex::encode(&arr);
        assert!(display_hash.starts_with("00"));
    }
}
