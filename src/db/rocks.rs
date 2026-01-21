//! RocksDB reader for Zebra state
//!
//! Reads directly from Zebra's RocksDB state database.
//! This is ~100-1000x faster than JSON-RPC calls.

use rocksdb::{DB, Options, IteratorMode};
use std::path::Path;
use std::time::Instant;
use crate::config::Config;
use crate::models::{Block, Transaction, TransparentInput, TransparentOutput};

/// Zebra column families we care about
pub const COLUMN_FAMILIES: &[&str] = &[
    "hash_by_height",
    "height_by_hash",
    "block_header_by_height",
    "tx_by_loc",
    "hash_by_tx_loc",
    "tx_loc_by_hash",
    "balance_by_transparent_addr",
    "tx_loc_by_transparent_addr_loc",
    "utxo_by_out_loc",
    "utxo_loc_by_transparent_addr_loc",
    "sprout_nullifiers",
    "sapling_nullifiers",
    "orchard_nullifiers",
    "sprout_anchors",
    "sapling_anchors",
    "orchard_anchors",
];

/// Wrapper around Zebra's RocksDB state
pub struct ZebraState {
    db: DB,
    config: Config,
}

impl ZebraState {
    /// Open Zebra state in read-only mode
    pub fn open(config: &Config) -> Result<Self, String> {
        let path = &config.zebra_state_path;

        if !path.exists() {
            return Err(format!("Zebra state not found at: {:?}", path));
        }

        let mut opts = Options::default();
        opts.set_error_if_exists(false);
        opts.create_if_missing(false);
        opts.set_max_open_files(config.max_open_files);

        // Get actual column families from database
        let cf_names = DB::list_cf(&Options::default(), path)
            .map_err(|e| format!("Failed to list column families: {}", e))?;

        let start = Instant::now();
        let db = DB::open_cf_for_read_only(&opts, path, &cf_names, false)
            .map_err(|e| format!("Failed to open RocksDB: {}", e))?;

        tracing::info!("RocksDB opened in {:?}", start.elapsed());

        Ok(Self {
            db,
            config: config.clone(),
        })
    }

    /// Get current chain tip height
    pub fn get_tip_height(&self) -> Result<u32, String> {
        let cf = self.db.cf_handle("hash_by_height")
            .ok_or("hash_by_height CF not found")?;

        let mut last_height = 0u32;

        // Iterate to find last entry (RocksDB is sorted)
        for item in self.db.iterator_cf(cf, IteratorMode::End) {
            match item {
                Ok((key, _)) => {
                    if key.len() >= 3 {
                        // 3-byte big-endian height
                        last_height = ((key[0] as u32) << 16)
                            | ((key[1] as u32) << 8)
                            | (key[2] as u32);
                    }
                    break;  // Only need the last one
                }
                Err(e) => return Err(format!("Error reading tip: {}", e)),
            }
        }

        Ok(last_height)
    }

    /// Get block hash by height
    pub fn get_block_hash(&self, height: u32) -> Result<[u8; 32], String> {
        let cf = self.db.cf_handle("hash_by_height")
            .ok_or("hash_by_height CF not found")?;

        // Encode height as 3-byte big-endian
        let key = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
        ];

        match self.db.get_cf(cf, &key) {
            Ok(Some(value)) => {
                if value.len() >= 32 {
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&value[..32]);
                    Ok(hash)
                } else {
                    Err(format!("Invalid hash length: {}", value.len()))
                }
            }
            Ok(None) => Err(format!("Block not found at height {}", height)),
            Err(e) => Err(format!("Error reading block hash: {}", e)),
        }
    }

    /// Iterate over all blocks from start_height to end_height
    pub fn iter_blocks(&self, start_height: u32, end_height: u32)
        -> impl Iterator<Item = Result<(u32, [u8; 32]), String>> + '_
    {
        let cf = self.db.cf_handle("hash_by_height");

        // Create starting key (3-byte big-endian)
        let start_key = [
            ((start_height >> 16) & 0xFF) as u8,
            ((start_height >> 8) & 0xFF) as u8,
            (start_height & 0xFF) as u8,
        ];

        let iter = if let Some(cf) = cf {
            Some(self.db.iterator_cf(cf, IteratorMode::From(&start_key, rocksdb::Direction::Forward)))
        } else {
            None
        };

        iter.into_iter()
            .flatten()
            .take_while(move |result| {
                match result {
                    Ok((key, _)) => {
                        if key.len() >= 3 {
                            let height = ((key[0] as u32) << 16)
                                | ((key[1] as u32) << 8)
                                | (key[2] as u32);
                            height <= end_height
                        } else {
                            false
                        }
                    }
                    Err(_) => false,
                }
            })
            .map(|result| {
                match result {
                    Ok((key, value)) => {
                        if key.len() >= 3 && value.len() >= 32 {
                            let height = ((key[0] as u32) << 16)
                                | ((key[1] as u32) << 8)
                                | (key[2] as u32);
                            let mut hash = [0u8; 32];
                            hash.copy_from_slice(&value[..32]);
                            Ok((height, hash))
                        } else {
                            Err("Invalid key/value length".to_string())
                        }
                    }
                    Err(e) => Err(format!("RocksDB error: {}", e)),
                }
            })
    }

    /// Get transaction by location (block height + tx index)
    pub fn get_transaction_by_loc(&self, height: u32, tx_index: u16) -> Result<Vec<u8>, String> {
        let cf = self.db.cf_handle("tx_by_loc")
            .ok_or("tx_by_loc CF not found")?;

        // Encode location: 3-byte height BE + 2-byte tx_index BE
        let key = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
            ((tx_index >> 8) & 0xFF) as u8,
            (tx_index & 0xFF) as u8,
        ];

        match self.db.get_cf(cf, &key) {
            Ok(Some(value)) => Ok(value.to_vec()),
            Ok(None) => Err(format!("Transaction not found at {}:{}", height, tx_index)),
            Err(e) => Err(format!("Error reading transaction: {}", e)),
        }
    }

    /// Get transaction hash by location
    pub fn get_tx_hash_by_loc(&self, height: u32, tx_index: u16) -> Result<[u8; 32], String> {
        let cf = self.db.cf_handle("hash_by_tx_loc")
            .ok_or("hash_by_tx_loc CF not found")?;

        // Same key format as tx_by_loc
        let key = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
            ((tx_index >> 8) & 0xFF) as u8,
            (tx_index & 0xFF) as u8,
        ];

        match self.db.get_cf(cf, &key) {
            Ok(Some(value)) => {
                if value.len() >= 32 {
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&value[..32]);
                    Ok(hash)
                } else {
                    Err(format!("Invalid hash length: {}", value.len()))
                }
            }
            Ok(None) => Err(format!("TX hash not found at {}:{}", height, tx_index)),
            Err(e) => Err(format!("Error reading tx hash: {}", e)),
        }
    }

    /// Iterate over all transactions in a block
    /// Returns (tx_index, raw_tx_bytes) for each transaction
    pub fn iter_block_transactions(&self, height: u32) -> Result<Vec<(u16, Vec<u8>)>, String> {
        let cf = self.db.cf_handle("tx_by_loc")
            .ok_or("tx_by_loc CF not found")?;

        // Prefix for this block height (3 bytes BE)
        let prefix = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
        ];

        let mut transactions = Vec::new();

        // Iterate from the start of this height's prefix
        for item in self.db.prefix_iterator_cf(cf, &prefix) {
            match item {
                Ok((key, value)) => {
                    // Check if still in same block (first 3 bytes match)
                    if key.len() >= 5 && key[0..3] == prefix {
                        let tx_index = ((key[3] as u16) << 8) | (key[4] as u16);
                        transactions.push((tx_index, value.to_vec()));
                    } else {
                        // Moved to next block, stop
                        break;
                    }
                }
                Err(e) => return Err(format!("Error iterating transactions: {}", e)),
            }
        }

        Ok(transactions)
    }

    /// Get count of transactions in a block
    pub fn get_block_tx_count(&self, height: u32) -> Result<u16, String> {
        let txs = self.iter_block_transactions(height)?;
        Ok(txs.len() as u16)
    }

    /// Count entries in a column family
    pub fn count_cf_entries(&self, cf_name: &str, limit: usize) -> usize {
        let cf = match self.db.cf_handle(cf_name) {
            Some(cf) => cf,
            None => return 0,
        };

        self.db.iterator_cf(cf, IteratorMode::Start)
            .take(limit)
            .count()
    }

    /// Get statistics about the database
    pub fn get_stats(&self) -> DbStats {
        let tip_height = self.get_tip_height().unwrap_or(0);

        DbStats {
            tip_height,
            block_count: tip_height + 1,
            network: self.config.network_name().to_string(),
        }
    }
}

/// Database statistics
#[derive(Debug)]
pub struct DbStats {
    pub tip_height: u32,
    pub block_count: u32,
    pub network: String,
}

#[cfg(test)]
mod tests {
    // Tests would go here
}
