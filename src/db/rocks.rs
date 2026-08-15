//! RocksDB reader for Zebra state
//!
//! Reads directly from Zebra's RocksDB state database.
//! This is ~100-1000x faster than JSON-RPC calls.
//!
//! Uses RocksDB secondary mode to follow Zebra's writes in real-time.

use crate::config::Config;
use rocksdb::{IteratorMode, Options, DB};
use std::io::Cursor;
use std::time::Instant;
use zebra_chain::block::Header as ZebraHeader;
use zebra_chain::serialization::ZcashDeserialize;

/// Wrapper around Zebra's RocksDB state
pub struct ZebraState {
    db: DB,
    config: Config,
    _secondary_path: std::path::PathBuf,
}

impl ZebraState {
    /// Open Zebra state in secondary mode (can follow primary writes)
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

        // RocksDB secondary instances require an exclusive secondary directory.
        // Include the PID so bounded parallel backfill workers can safely read the
        // same Zebra primary without sharing lock or catch-up state.
        let secondary_path =
            std::env::temp_dir().join(format!("cipherscan-rocks-secondary-{}", std::process::id()));
        std::fs::create_dir_all(&secondary_path)
            .map_err(|e| format!("Failed to create secondary path: {}", e))?;

        let start = Instant::now();
        let db = DB::open_cf_as_secondary(&opts, path, &secondary_path, &cf_names)
            .map_err(|e| format!("Failed to open RocksDB as secondary: {}", e))?;

        tracing::info!("RocksDB opened in {:?}", start.elapsed());

        Ok(Self {
            db,
            config: config.clone(),
            _secondary_path: secondary_path,
        })
    }

    /// Get current chain tip height
    pub fn get_tip_height(&self) -> Result<u32, String> {
        let cf = self
            .db
            .cf_handle("hash_by_height")
            .ok_or("hash_by_height CF not found")?;

        let mut last_height = 0u32;

        // RocksDB is sorted; only the last entry is needed.
        if let Some(item) = self.db.iterator_cf(cf, IteratorMode::End).next() {
            let (key, _) = item.map_err(|e| format!("Error reading tip: {}", e))?;
            if key.len() >= 3 {
                // 3-byte big-endian height
                last_height = ((key[0] as u32) << 16) | ((key[1] as u32) << 8) | (key[2] as u32);
            }
        }

        Ok(last_height)
    }

    /// Get block hash by height
    pub fn get_block_hash(&self, height: u32) -> Result<[u8; 32], String> {
        let cf = self
            .db
            .cf_handle("hash_by_height")
            .ok_or("hash_by_height CF not found")?;

        // Encode height as 3-byte big-endian
        let key = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
        ];

        match self.db.get_cf(cf, key) {
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

    /// Get and parse block header
    pub fn get_block_header(&self, height: u32) -> Result<ParsedBlockHeader, String> {
        let cf = self
            .db
            .cf_handle("block_header_by_height")
            .ok_or("block_header_by_height CF not found")?;

        // Encode height as 3-byte big-endian
        let key = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
        ];

        match self.db.get_cf(cf, key) {
            Ok(Some(value)) => {
                // Parse header using zebra-chain
                let mut cursor = Cursor::new(&value[..]);
                let header = ZebraHeader::zcash_deserialize(&mut cursor)
                    .map_err(|e| format!("Failed to parse header: {:?}", e))?;

                // Extract version (use Debug format to get the inner value)
                let version = {
                    let v_str = format!("{:?}", header.version);
                    // Parse "Version(4)" -> 4
                    v_str
                        .trim_start_matches("Version(")
                        .trim_end_matches(')')
                        .parse::<i32>()
                        .unwrap_or(4)
                };

                // Convert hashes to hex (display order = reversed)
                let prev_hash = {
                    let mut rev = header.previous_block_hash.0;
                    rev.reverse();
                    hex::encode(rev)
                };

                let merkle_root = {
                    let mut rev = header.merkle_root.0;
                    rev.reverse();
                    hex::encode(rev)
                };

                let final_sapling_root = {
                    let mut rev = header.commitment_bytes.0;
                    rev.reverse();
                    hex::encode(rev)
                };

                // Time
                let time = header.time.timestamp() as u64;

                // Difficulty/bits - extract from Debug format
                let (bits, difficulty) = {
                    let bits_str = format!("{:?}", header.difficulty_threshold);
                    // Format is like "CompactDifficulty(0x1c00f2d4, Some(...))"
                    // Extract just the hex value
                    let bits_hex = if let Some(start) = bits_str.find("0x") {
                        let after_0x = &bits_str[start + 2..];
                        if let Some(end) = after_0x.find(|c: char| !c.is_ascii_hexdigit()) {
                            after_0x[..end].to_string()
                        } else {
                            after_0x.to_string()
                        }
                    } else {
                        String::new()
                    };

                    let bits_val = u32::from_str_radix(&bits_hex, 16).unwrap_or(0);
                    let diff = Self::compact_to_difficulty(bits_val);
                    (bits_hex, diff)
                };

                // Nonce - extract hex from HexDebug format and reverse bytes for standard display
                let nonce = {
                    let nonce_str = format!("{:?}", header.nonce);
                    // Format is like '[u8; 32]("5800d153...")'
                    let hex_str = if let Some(start) = nonce_str.find('"') {
                        if let Some(end) = nonce_str.rfind('"') {
                            nonce_str[start + 1..end].to_string()
                        } else {
                            nonce_str.clone()
                        }
                    } else {
                        nonce_str.clone()
                    };

                    // Reverse byte order for standard display (little-endian to big-endian)
                    if hex_str.len() == 64 {
                        let bytes: Vec<u8> = (0..32)
                            .filter_map(|i| u8::from_str_radix(&hex_str[i * 2..i * 2 + 2], 16).ok())
                            .collect();
                        let reversed: Vec<u8> = bytes.into_iter().rev().collect();
                        hex::encode(&reversed)
                    } else {
                        hex_str
                    }
                };

                // Solution - extract hex, store truncated
                let solution = {
                    let sol_str = format!("{:?}", header.solution);
                    if let Some(start) = sol_str.find('"') {
                        if let Some(end) = sol_str.rfind('"') {
                            sol_str[start + 1..end].to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                };

                Ok(ParsedBlockHeader {
                    version,
                    previous_block_hash: prev_hash,
                    merkle_root,
                    final_sapling_root,
                    final_orchard_root: None,
                    final_ironwood_root: None,
                    time,
                    bits,
                    difficulty,
                    nonce,
                    solution,
                })
            }
            Ok(None) => Err(format!("Block header not found at height {}", height)),
            Err(e) => Err(format!("Error reading block header: {}", e)),
        }
    }

    /// Convert compact difficulty (nBits) to full difficulty
    fn compact_to_difficulty(compact: u32) -> f64 {
        let exponent = (compact >> 24) as i32;
        let mantissa = (compact & 0x00ffffff) as f64;

        if mantissa == 0.0 {
            return 0.0;
        }

        // Difficulty = max_target / current_target
        // For Zcash, max_target has exponent 0x1f and mantissa 0x07ffff
        let max_target = 0x07ffff as f64 * 256.0_f64.powi(0x1f - 3);
        let current_target = mantissa * 256.0_f64.powi(exponent - 3);

        max_target / current_target
    }

    /// Get transaction by location (block height + tx index)
    pub fn get_transaction_by_loc(&self, height: u32, tx_index: u16) -> Result<Vec<u8>, String> {
        let cf = self
            .db
            .cf_handle("tx_by_loc")
            .ok_or("tx_by_loc CF not found")?;

        // Encode location: 3-byte height BE + 2-byte tx_index BE
        let key = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
            ((tx_index >> 8) & 0xFF) as u8,
            (tx_index & 0xFF) as u8,
        ];

        match self.db.get_cf(cf, key) {
            Ok(Some(value)) => Ok(value.to_vec()),
            Ok(None) => Err(format!("Transaction not found at {}:{}", height, tx_index)),
            Err(e) => Err(format!("Error reading transaction: {}", e)),
        }
    }

    /// Get transaction hash by location
    pub fn get_tx_hash_by_loc(&self, height: u32, tx_index: u16) -> Result<[u8; 32], String> {
        let cf = self
            .db
            .cf_handle("hash_by_tx_loc")
            .ok_or("hash_by_tx_loc CF not found")?;

        // Same key format as tx_by_loc
        let key = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
            ((tx_index >> 8) & 0xFF) as u8,
            (tx_index & 0xFF) as u8,
        ];

        match self.db.get_cf(cf, key) {
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
        let cf = self
            .db
            .cf_handle("tx_by_loc")
            .ok_or("tx_by_loc CF not found")?;

        // Prefix for this block height (3 bytes BE)
        let prefix = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
        ];

        let mut transactions = Vec::new();

        // Iterate from the start of this height's prefix
        for item in self.db.prefix_iterator_cf(cf, prefix) {
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

    /// Get transaction location (height, index) by txid hash
    /// The txid should be in internal byte order (not display order)
    pub fn get_tx_loc_by_hash(&self, txid_bytes: &[u8; 32]) -> Result<(u32, u16), String> {
        let cf = self
            .db
            .cf_handle("tx_loc_by_hash")
            .ok_or("tx_loc_by_hash CF not found")?;

        match self.db.get_cf(cf, txid_bytes) {
            Ok(Some(value)) => {
                if value.len() >= 5 {
                    // 3-byte height BE + 2-byte tx_index BE
                    let height =
                        ((value[0] as u32) << 16) | ((value[1] as u32) << 8) | (value[2] as u32);
                    let tx_index = ((value[3] as u16) << 8) | (value[4] as u16);
                    Ok((height, tx_index))
                } else {
                    Err(format!("Invalid tx_loc length: {}", value.len()))
                }
            }
            Ok(None) => Err("Transaction not found by hash".to_string()),
            Err(e) => Err(format!("Error looking up tx by hash: {}", e)),
        }
    }

    /// Get a previous output's value and address using UTXO lookup (fast path)
    /// Falls back to parsing the full transaction if UTXO not found (already spent)
    /// Returns (value_zat, address_option)
    pub fn get_prev_output(
        &self,
        prev_txid_hex: &str,
        prev_vout: u32,
    ) -> Result<(i64, Option<String>), String> {
        // Convert hex txid to bytes (internal order - reversed)
        let txid_bytes =
            hex::decode(prev_txid_hex).map_err(|e| format!("Invalid txid hex: {}", e))?;

        if txid_bytes.len() != 32 {
            return Err(format!("Invalid txid length: {}", txid_bytes.len()));
        }

        // Reverse for internal byte order (Zcash stores in internal order)
        let mut txid_internal = [0u8; 32];
        for (i, b) in txid_bytes.iter().enumerate() {
            txid_internal[31 - i] = *b;
        }

        // Look up the transaction location
        let (height, tx_index) = self.get_tx_loc_by_hash(&txid_internal)?;

        // Try UTXO lookup first (fast path - only works for unspent outputs)
        if let Ok(Some((value, address))) = self.get_utxo_by_loc(height, tx_index, prev_vout as u16)
        {
            return Ok((value, address));
        }

        // Fallback: parse the full transaction (slower, but works for spent outputs)
        self.get_output_by_parsing(height, tx_index, prev_vout)
    }

    /// Get UTXO directly from utxo_by_out_loc (fast, but only for unspent)
    fn get_utxo_by_loc(
        &self,
        height: u32,
        tx_index: u16,
        output_index: u16,
    ) -> Result<Option<(i64, Option<String>)>, String> {
        let cf = self
            .db
            .cf_handle("utxo_by_out_loc")
            .ok_or("utxo_by_out_loc CF not found")?;

        // Key: 3-byte height BE + 2-byte tx_index BE + 2-byte output_index BE
        let key = [
            ((height >> 16) & 0xFF) as u8,
            ((height >> 8) & 0xFF) as u8,
            (height & 0xFF) as u8,
            ((tx_index >> 8) & 0xFF) as u8,
            (tx_index & 0xFF) as u8,
            ((output_index >> 8) & 0xFF) as u8,
            (output_index & 0xFF) as u8,
        ];

        match self.db.get_cf(cf, key) {
            Ok(Some(value)) => {
                // Parse the UTXO value: 8-byte value LE + script
                if value.len() < 8 {
                    return Ok(None);
                }

                let amount = i64::from_le_bytes(value[0..8].try_into().unwrap());
                let script = &value[8..];

                // Parse address from script
                let address = self.parse_address_from_script(script);

                Ok(Some((amount, address)))
            }
            Ok(None) => Ok(None), // UTXO not found (already spent)
            Err(e) => Err(format!("UTXO lookup error: {}", e)),
        }
    }

    /// Parse address from raw script bytes
    fn parse_address_from_script(&self, script: &[u8]) -> Option<String> {
        use crate::config::Network;

        let (p2pkh_prefix, p2sh_prefix) = match self.config.network {
            Network::Mainnet => ([0x1C, 0xB8], [0x1C, 0xBD]), // t1, t3
            Network::Testnet => ([0x1D, 0x25], [0x1C, 0xBA]), // tm, t2
        };

        // P2PKH: OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG
        if script.len() == 25
            && script[0] == 0x76  // OP_DUP
            && script[1] == 0xa9  // OP_HASH160
            && script[2] == 0x14  // Push 20 bytes
            && script[23] == 0x88 // OP_EQUALVERIFY
            && script[24] == 0xac
        // OP_CHECKSIG
        {
            let hash = &script[3..23];
            return Some(Self::encode_address_static(&p2pkh_prefix, hash));
        }

        // P2SH: OP_HASH160 <20 bytes> OP_EQUAL
        if script.len() == 23
            && script[0] == 0xa9  // OP_HASH160
            && script[1] == 0x14  // Push 20 bytes
            && script[22] == 0x87
        // OP_EQUAL
        {
            let hash = &script[2..22];
            return Some(Self::encode_address_static(&p2sh_prefix, hash));
        }

        None
    }

    /// Encode address with Base58Check
    fn encode_address_static(prefix: &[u8], hash: &[u8]) -> String {
        use sha2::{Digest, Sha256};

        let mut data = Vec::with_capacity(prefix.len() + hash.len() + 4);
        data.extend_from_slice(prefix);
        data.extend_from_slice(hash);

        let first = Sha256::digest(&data);
        let second = Sha256::digest(first);
        data.extend_from_slice(&second[0..4]);

        bs58::encode(&data).into_string()
    }

    /// Fallback: parse the full transaction to get output (slower)
    fn get_output_by_parsing(
        &self,
        height: u32,
        tx_index: u16,
        output_index: u32,
    ) -> Result<(i64, Option<String>), String> {
        use crate::indexer::TransactionParser;

        let raw_tx = self.get_transaction_by_loc(height, tx_index)?;

        let block_hash = {
            let mut h = self.get_block_hash(height)?;
            h.reverse();
            hex::encode(h)
        };

        let tx = TransactionParser::parse(&raw_tx, height, &block_hash, self.config.network)?;

        if let Some(output) = tx.vout.get(output_index as usize) {
            Ok((output.value, output.address.clone()))
        } else {
            Err(format!("Output {} not found in tx", output_index))
        }
    }
}

/// Parsed block header with all fields
#[derive(Debug, Clone)]
pub struct ParsedBlockHeader {
    pub version: i32,
    pub previous_block_hash: String,
    pub merkle_root: String,
    pub final_sapling_root: String,
    pub final_orchard_root: Option<String>,
    pub final_ironwood_root: Option<String>,
    pub time: u64,
    pub bits: String,
    pub difficulty: f64,
    pub nonce: String,
    pub solution: String,
}
