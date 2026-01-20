//! Transaction parsing logic
//!
//! Parses raw transaction bytes from Zebra's RocksDB into structured Transaction objects.

use crate::models::{Transaction, TransparentInput, TransparentOutput};

/// Transaction parser
pub struct TransactionParser;

impl TransactionParser {
    /// Parse a raw transaction from bytes
    /// 
    /// Zcash transaction format varies by version:
    /// - v1-v2: Standard Bitcoin-like
    /// - v3: Overwinter (with expiry)
    /// - v4: Sapling
    /// - v5: NU5/Orchard
    pub fn parse(raw: &[u8], block_height: u32, block_hash: &str) -> Result<Transaction, String> {
        if raw.len() < 4 {
            return Err("Transaction too short".to_string());
        }
        
        // First 4 bytes: version + overwintered flag
        let header = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let version = (header & 0x7FFFFFFF) as i32;
        let overwintered = (header >> 31) == 1;
        
        // Compute txid (double SHA256 of serialized tx, reversed)
        let txid = Self::compute_txid(raw);
        
        // Parse based on version
        match version {
            1 | 2 => Self::parse_v1_v2(raw, version, block_height, block_hash, &txid),
            3 => Self::parse_v3(raw, block_height, block_hash, &txid),
            4 => Self::parse_v4(raw, block_height, block_hash, &txid),
            5 => Self::parse_v5(raw, block_height, block_hash, &txid),
            _ => Err(format!("Unknown transaction version: {}", version)),
        }
    }
    
    /// Compute transaction ID
    fn compute_txid(raw: &[u8]) -> String {
        use sha2::{Sha256, Digest};
        
        let first = Sha256::digest(raw);
        let second = Sha256::digest(&first);
        
        // Reverse for display
        let mut txid = second.to_vec();
        txid.reverse();
        hex::encode(&txid)
    }
    
    /// Parse v1/v2 transaction (legacy)
    fn parse_v1_v2(raw: &[u8], version: i32, block_height: u32, block_hash: &str, txid: &str) -> Result<Transaction, String> {
        let mut offset = 4; // Skip version
        
        // vin count (varint)
        let (vin_count, consumed) = Self::read_varint(&raw[offset..])?;
        offset += consumed;
        
        // Parse vins
        let mut vin = Vec::new();
        let mut transparent_value_in = 0i64;
        
        for _ in 0..vin_count {
            let (input, consumed) = Self::parse_vin(&raw[offset..])?;
            offset += consumed;
            vin.push(input);
        }
        
        // vout count
        let (vout_count, consumed) = Self::read_varint(&raw[offset..])?;
        offset += consumed;
        
        // Parse vouts
        let mut vout = Vec::new();
        let mut transparent_value_out = 0i64;
        
        for n in 0..vout_count {
            let (output, consumed) = Self::parse_vout(&raw[offset..], n as u32)?;
            offset += consumed;
            transparent_value_out += output.value;
            vout.push(output);
        }
        
        // lock_time (4 bytes)
        let lock_time = if offset + 4 <= raw.len() {
            u32::from_le_bytes([raw[offset], raw[offset+1], raw[offset+2], raw[offset+3]])
        } else {
            0
        };
        
        Ok(Transaction {
            txid: txid.to_string(),
            block_height,
            block_hash: block_hash.to_string(),
            version,
            lock_time,
            expiry_height: None,
            size: raw.len() as u32,
            vin_count: vin_count as u16,
            vout_count: vout_count as u16,
            transparent_value_in,
            transparent_value_out,
            joinsplit_count: 0,
            sapling_spends: 0,
            sapling_outputs: 0,
            orchard_actions: 0,
            sapling_value_balance: 0,
            orchard_value_balance: 0,
            fee: None,
            vin,
            vout,
        })
    }
    
    /// Parse v3 transaction (Overwinter)
    fn parse_v3(raw: &[u8], block_height: u32, block_hash: &str, txid: &str) -> Result<Transaction, String> {
        // Similar to v1/v2 but with expiry_height
        // Simplified for now
        Self::parse_v1_v2(raw, 3, block_height, block_hash, txid)
    }
    
    /// Parse v4 transaction (Sapling)
    fn parse_v4(raw: &[u8], block_height: u32, block_hash: &str, txid: &str) -> Result<Transaction, String> {
        let mut offset = 4; // Skip version header
        
        // Version group ID (4 bytes)
        offset += 4;
        
        // vin count
        let (vin_count, consumed) = Self::read_varint(&raw[offset..])?;
        offset += consumed;
        
        // Parse vins (simplified)
        for _ in 0..vin_count {
            let (_, consumed) = Self::parse_vin(&raw[offset..])?;
            offset += consumed;
        }
        
        // vout count
        let (vout_count, consumed) = Self::read_varint(&raw[offset..])?;
        offset += consumed;
        
        // Parse vouts
        let mut vout = Vec::new();
        let mut transparent_value_out = 0i64;
        
        for n in 0..vout_count {
            let (output, consumed) = Self::parse_vout(&raw[offset..], n as u32)?;
            offset += consumed;
            transparent_value_out += output.value;
            vout.push(output);
        }
        
        // lock_time (4 bytes)
        let lock_time = u32::from_le_bytes([raw[offset], raw[offset+1], raw[offset+2], raw[offset+3]]);
        offset += 4;
        
        // expiry_height (4 bytes)
        let expiry_height = u32::from_le_bytes([raw[offset], raw[offset+1], raw[offset+2], raw[offset+3]]);
        offset += 4;
        
        // value_balance (8 bytes, Sapling)
        let sapling_value_balance = if offset + 8 <= raw.len() {
            i64::from_le_bytes([
                raw[offset], raw[offset+1], raw[offset+2], raw[offset+3],
                raw[offset+4], raw[offset+5], raw[offset+6], raw[offset+7]
            ])
        } else {
            0
        };
        offset += 8;
        
        // Sapling spends/outputs counts (would need to parse further)
        // For now, infer from value_balance
        let sapling_spends = if sapling_value_balance > 0 { 1 } else { 0 };
        let sapling_outputs = if sapling_value_balance < 0 { 1 } else { 0 };
        
        Ok(Transaction {
            txid: txid.to_string(),
            block_height,
            block_hash: block_hash.to_string(),
            version: 4,
            lock_time,
            expiry_height: Some(expiry_height),
            size: raw.len() as u32,
            vin_count: vin_count as u16,
            vout_count: vout_count as u16,
            transparent_value_in: 0,
            transparent_value_out,
            joinsplit_count: 0,
            sapling_spends,
            sapling_outputs,
            orchard_actions: 0,
            sapling_value_balance,
            orchard_value_balance: 0,
            fee: None,
            vin: Vec::new(),
            vout,
        })
    }
    
    /// Parse v5 transaction (NU5/Orchard)
    fn parse_v5(raw: &[u8], block_height: u32, block_hash: &str, txid: &str) -> Result<Transaction, String> {
        // V5 format is different - uses ZIP-225 encoding
        // Header: version (4) + version_group_id (4) + consensus_branch_id (4)
        // + lock_time (4) + expiry_height (4)
        
        if raw.len() < 20 {
            return Err("V5 transaction too short".to_string());
        }
        
        let mut offset = 4; // version
        offset += 4; // version_group_id
        offset += 4; // consensus_branch_id
        
        let lock_time = u32::from_le_bytes([raw[offset], raw[offset+1], raw[offset+2], raw[offset+3]]);
        offset += 4;
        
        let expiry_height = u32::from_le_bytes([raw[offset], raw[offset+1], raw[offset+2], raw[offset+3]]);
        offset += 4;
        
        // vin count
        let (vin_count, consumed) = Self::read_varint(&raw[offset..])?;
        offset += consumed;
        
        // Skip vins
        for _ in 0..vin_count {
            let (_, consumed) = Self::parse_vin(&raw[offset..])?;
            offset += consumed;
        }
        
        // vout count
        let (vout_count, consumed) = Self::read_varint(&raw[offset..])?;
        offset += consumed;
        
        // Parse vouts
        let mut vout = Vec::new();
        let mut transparent_value_out = 0i64;
        
        for n in 0..vout_count {
            if offset >= raw.len() { break; }
            let (output, consumed) = Self::parse_vout(&raw[offset..], n as u32)?;
            offset += consumed;
            transparent_value_out += output.value;
            vout.push(output);
        }
        
        // Sapling and Orchard sections follow...
        // This is simplified - full parsing is more complex
        
        Ok(Transaction {
            txid: txid.to_string(),
            block_height,
            block_hash: block_hash.to_string(),
            version: 5,
            lock_time,
            expiry_height: Some(expiry_height),
            size: raw.len() as u32,
            vin_count: vin_count as u16,
            vout_count: vout_count as u16,
            transparent_value_in: 0,
            transparent_value_out,
            joinsplit_count: 0,
            sapling_spends: 0,
            sapling_outputs: 0,
            orchard_actions: 0,  // Would need full parsing
            sapling_value_balance: 0,
            orchard_value_balance: 0,
            fee: None,
            vin: Vec::new(),
            vout,
        })
    }
    
    /// Read a Bitcoin-style varint
    fn read_varint(data: &[u8]) -> Result<(u64, usize), String> {
        if data.is_empty() {
            return Err("Empty data for varint".to_string());
        }
        
        let first = data[0];
        match first {
            0..=252 => Ok((first as u64, 1)),
            253 => {
                if data.len() < 3 {
                    return Err("Varint too short".to_string());
                }
                Ok((u16::from_le_bytes([data[1], data[2]]) as u64, 3))
            }
            254 => {
                if data.len() < 5 {
                    return Err("Varint too short".to_string());
                }
                Ok((u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as u64, 5))
            }
            255 => {
                if data.len() < 9 {
                    return Err("Varint too short".to_string());
                }
                Ok((u64::from_le_bytes([
                    data[1], data[2], data[3], data[4],
                    data[5], data[6], data[7], data[8]
                ]), 9))
            }
        }
    }
    
    /// Parse a transparent input
    fn parse_vin(data: &[u8]) -> Result<(TransparentInput, usize), String> {
        if data.len() < 36 {
            return Err("vin too short".to_string());
        }
        
        let mut offset = 0;
        
        // Previous txid (32 bytes)
        let mut prev_txid = [0u8; 32];
        prev_txid.copy_from_slice(&data[0..32]);
        prev_txid.reverse();
        offset += 32;
        
        // Previous vout index (4 bytes)
        let vout = u32::from_le_bytes([data[32], data[33], data[34], data[35]]);
        offset += 4;
        
        // Check for coinbase
        let is_coinbase = prev_txid.iter().all(|&b| b == 0) && vout == 0xFFFFFFFF;
        
        // Script length (varint)
        let (script_len, consumed) = Self::read_varint(&data[offset..])?;
        offset += consumed;
        
        // Script
        let script_end = offset + script_len as usize;
        if script_end > data.len() {
            return Err("Script extends beyond data".to_string());
        }
        offset = script_end;
        
        // Sequence (4 bytes)
        if offset + 4 > data.len() {
            return Err("Missing sequence".to_string());
        }
        offset += 4;
        
        Ok((TransparentInput {
            txid: hex::encode(&prev_txid),
            vout,
            address: None,  // Would need script analysis
            value: None,
            is_coinbase,
            script_sig: None,
        }, offset))
    }
    
    /// Parse a transparent output
    fn parse_vout(data: &[u8], n: u32) -> Result<(TransparentOutput, usize), String> {
        if data.len() < 8 {
            return Err("vout too short".to_string());
        }
        
        let mut offset = 0;
        
        // Value (8 bytes)
        let value = i64::from_le_bytes([
            data[0], data[1], data[2], data[3],
            data[4], data[5], data[6], data[7]
        ]);
        offset += 8;
        
        // Script length (varint)
        let (script_len, consumed) = Self::read_varint(&data[offset..])?;
        offset += consumed;
        
        // Script
        let script_end = offset + script_len as usize;
        if script_end > data.len() {
            return Err("Script extends beyond data".to_string());
        }
        
        let script = &data[offset..script_end];
        let (address, script_type) = Self::decode_output_script(script);
        offset = script_end;
        
        Ok((TransparentOutput {
            n,
            value,
            address,
            script_type,
            script_pub_key: None,
        }, offset))
    }
    
    /// Decode output script to get address and type
    fn decode_output_script(script: &[u8]) -> (Option<String>, String) {
        if script.is_empty() {
            return (None, "empty".to_string());
        }
        
        // P2PKH: OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG
        if script.len() == 25 
            && script[0] == 0x76  // OP_DUP
            && script[1] == 0xa9  // OP_HASH160
            && script[2] == 0x14  // Push 20 bytes
            && script[23] == 0x88 // OP_EQUALVERIFY
            && script[24] == 0xac // OP_CHECKSIG
        {
            let hash = &script[3..23];
            // Mainnet t1 address prefix: [0x1C, 0xB8]
            let address = Self::encode_address(&[0x1C, 0xB8], hash);
            return (Some(address), "pubkeyhash".to_string());
        }
        
        // P2SH: OP_HASH160 <20 bytes> OP_EQUAL
        if script.len() == 23
            && script[0] == 0xa9  // OP_HASH160
            && script[1] == 0x14  // Push 20 bytes
            && script[22] == 0x87 // OP_EQUAL
        {
            let hash = &script[2..22];
            // Mainnet t3 address prefix: [0x1C, 0xBD]
            let address = Self::encode_address(&[0x1C, 0xBD], hash);
            return (Some(address), "scripthash".to_string());
        }
        
        // OP_RETURN
        if !script.is_empty() && script[0] == 0x6a {
            return (None, "nulldata".to_string());
        }
        
        (None, "nonstandard".to_string())
    }
    
    /// Encode address with Base58Check
    fn encode_address(prefix: &[u8], hash: &[u8]) -> String {
        use sha2::{Sha256, Digest};
        
        let mut data = Vec::with_capacity(prefix.len() + hash.len() + 4);
        data.extend_from_slice(prefix);
        data.extend_from_slice(hash);
        
        // Checksum: first 4 bytes of double SHA256
        let first = Sha256::digest(&data);
        let second = Sha256::digest(&first);
        data.extend_from_slice(&second[0..4]);
        
        bs58::encode(&data).into_string()
    }
}
