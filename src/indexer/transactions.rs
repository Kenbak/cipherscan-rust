//! Transaction parsing logic using zebra-chain
//!
//! Uses zebra-chain's native deserialization for proper parsing of all tx versions.

use std::io::Cursor;
use zebra_chain::serialization::ZcashDeserialize;
use zebra_chain::transaction::Transaction as ZebraTransaction;
use crate::config::Network;
use crate::models::{Transaction, TransparentInput, TransparentOutput};

/// Transaction parser using zebra-chain
pub struct TransactionParser;

impl TransactionParser {
    /// Get address version prefixes for the given network
    fn addr_prefixes(network: Network) -> ([u8; 2], [u8; 2]) {
        match network {
            Network::Mainnet => ([0x1C, 0xB8], [0x1C, 0xBD]), // t1, t3
            Network::Testnet => ([0x1D, 0x25], [0x1C, 0xBA]), // tm, t2
        }
    }

    /// Parse a raw transaction from bytes using zebra-chain
    pub fn parse(raw: &[u8], block_height: u32, block_hash: &str, network: Network) -> Result<Transaction, String> {
        // Use zebra-chain to deserialize
        let mut cursor = Cursor::new(raw);
        let zebra_tx = ZebraTransaction::zcash_deserialize(&mut cursor)
            .map_err(|e| format!("Failed to deserialize transaction: {:?}", e))?;

        // Convert to our Transaction type
        Self::from_zebra_tx(zebra_tx, block_height, block_hash, raw.len(), network)
    }

    /// Convert zebra-chain Transaction to our Transaction model
    fn from_zebra_tx(
        tx: ZebraTransaction,
        block_height: u32,
        block_hash: &str,
        size: usize,
        network: Network,
    ) -> Result<Transaction, String> {
        use zebra_chain::transaction::Transaction::*;

        // Get txid
        let txid = tx.hash().to_string();

        // Extract data based on transaction version
        // Use transmute-like conversion via serialization or raw access
        let (version, lock_time_raw, expiry_height_raw): (i32, u32, Option<u32>) = match &tx {
            V1 { .. } => (1, 0, None),
            V2 { .. } => (2, 0, None),
            V3 { expiry_height, .. } => (3, 0, Some(expiry_height.0)),
            V4 { expiry_height, .. } => (4, 0, Some(expiry_height.0)),
            V5 { expiry_height, .. } => (5, 0, Some(expiry_height.0)),
        };

        // Get transparent inputs/outputs
        let inputs = tx.inputs();
        let outputs = tx.outputs();

        // Parse transparent inputs
        let mut vin: Vec<TransparentInput> = Vec::new();
        let mut transparent_value_in: i64 = 0;
        let mut is_coinbase = false;

        for (i, input) in inputs.iter().enumerate() {
            use zebra_chain::transparent::Input;
            match input {
                Input::Coinbase { .. } => {
                    is_coinbase = true;
                    vin.push(TransparentInput {
                        txid: "0".repeat(64),
                        vout: 0xFFFFFFFF,
                        address: None,
                        value: None,
                        is_coinbase: true,
                        script_sig: None,
                    });
                }
                Input::PrevOut { outpoint, unlock_script, .. } => {
                    vin.push(TransparentInput {
                        txid: outpoint.hash.to_string(),
                        vout: outpoint.index,
                        address: None, // Would need UTXO lookup
                        value: None,   // Would need UTXO lookup
                        is_coinbase: false,
                        script_sig: Some(hex::encode(unlock_script.as_raw_bytes())),
                    });
                }
            }
        }

        // Parse transparent outputs
        let mut vout: Vec<TransparentOutput> = Vec::new();
        let mut transparent_value_out: i64 = 0;

        for (n, output) in outputs.iter().enumerate() {
            let value_zat = i64::from(output.value);
            transparent_value_out += value_zat;

            // Try to get address from lock script
            let (address, script_type) = Self::parse_output_script(&output.lock_script, network);

            vout.push(TransparentOutput {
                n: n as u32,
                value: value_zat,
                address,
                script_type,
                script_pub_key: Some(hex::encode(output.lock_script.as_raw_bytes())),
            });
        }

        // Get shielded data
        let (joinsplit_count, sapling_spends, sapling_outputs, orchard_actions) = match &tx {
            V1 { .. } | V2 { .. } => (0, 0, 0, 0),
            V3 { joinsplit_data, .. } => {
                let js_count = joinsplit_data.as_ref().map(|d| d.joinsplits().count()).unwrap_or(0);
                (js_count as u16, 0, 0, 0)
            }
            V4 { joinsplit_data, sapling_shielded_data, .. } => {
                let js_count = joinsplit_data.as_ref().map(|d| d.joinsplits().count()).unwrap_or(0);
                let (spends, outputs) = sapling_shielded_data.as_ref()
                    .map(|d| (d.spends().count(), d.outputs().count()))
                    .unwrap_or((0, 0));
                (js_count as u16, spends as u16, outputs as u16, 0)
            }
            V5 { sapling_shielded_data, orchard_shielded_data, .. } => {
                let (spends, outputs) = sapling_shielded_data.as_ref()
                    .map(|d| (d.spends().count(), d.outputs().count()))
                    .unwrap_or((0, 0));
                let actions = orchard_shielded_data.as_ref()
                    .map(|d| d.actions.len())
                    .unwrap_or(0);
                (0, spends as u16, outputs as u16, actions as u16)
            }
        };

        // Get value balances
        let sapling_value_balance: i64 = match &tx {
            V4 { sapling_shielded_data, .. } => {
                sapling_shielded_data.as_ref()
                    .map(|d| i64::from(d.value_balance))
                    .unwrap_or(0)
            }
            V5 { sapling_shielded_data, .. } => {
                sapling_shielded_data.as_ref()
                    .map(|d| i64::from(d.value_balance))
                    .unwrap_or(0)
            }
            _ => 0,
        };

        let orchard_value_balance: i64 = match &tx {
            V5 { orchard_shielded_data, .. } => {
                orchard_shielded_data.as_ref()
                    .map(|d| i64::from(d.value_balance))
                    .unwrap_or(0)
            }
            _ => 0,
        };

        // Calculate fee (for non-coinbase)
        let fee = if is_coinbase {
            None
        } else {
            // Fee = transparent_in + shielded_in - transparent_out - shielded_out
            // For now, we'd need UTXO values for transparent_in
            None
        };

        Ok(Transaction {
            txid,
            block_height,
            block_hash: block_hash.to_string(),
            version,
            lock_time: lock_time_raw,
            expiry_height: expiry_height_raw,
            size: size as u32,
            vin_count: vin.len() as u16,
            vout_count: vout.len() as u16,
            transparent_value_in,
            transparent_value_out,
            joinsplit_count,
            sapling_spends,
            sapling_outputs,
            orchard_actions,
            sapling_value_balance,
            orchard_value_balance,
            fee,
            vin,
            vout,
        })
    }

    /// Parse output script to get address and type
    fn parse_output_script(script: &zebra_chain::transparent::Script, network: Network) -> (Option<String>, String) {
        let bytes = script.as_raw_bytes();

        if bytes.is_empty() {
            return (None, "empty".to_string());
        }

        let (p2pkh_prefix, p2sh_prefix) = Self::addr_prefixes(network);

        // P2PKH: OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG
        if bytes.len() == 25
            && bytes[0] == 0x76  // OP_DUP
            && bytes[1] == 0xa9  // OP_HASH160
            && bytes[2] == 0x14  // Push 20 bytes
            && bytes[23] == 0x88 // OP_EQUALVERIFY
            && bytes[24] == 0xac // OP_CHECKSIG
        {
            let hash = &bytes[3..23];
            let address = Self::encode_address(&p2pkh_prefix, hash);
            return (Some(address), "pubkeyhash".to_string());
        }

        // P2SH: OP_HASH160 <20 bytes> OP_EQUAL
        if bytes.len() == 23
            && bytes[0] == 0xa9  // OP_HASH160
            && bytes[1] == 0x14  // Push 20 bytes
            && bytes[22] == 0x87 // OP_EQUAL
        {
            let hash = &bytes[2..22];
            let address = Self::encode_address(&p2sh_prefix, hash);
            return (Some(address), "scripthash".to_string());
        }

        // OP_RETURN
        if !bytes.is_empty() && bytes[0] == 0x6a {
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

        // Checksum
        let first = Sha256::digest(&data);
        let second = Sha256::digest(&first);
        data.extend_from_slice(&second[0..4]);

        bs58::encode(&data).into_string()
    }

    /// Resolve input addresses and values by looking up previous outputs
    /// This mutates the transaction in place, and calculates the fee
    pub fn resolve_inputs(tx: &mut Transaction, zebra: &crate::db::ZebraState) {
        // Skip coinbase - no inputs to resolve, no fee
        if tx.vin.iter().any(|v| v.is_coinbase) {
            tx.fee = None;
            return;
        }

        for input in tx.vin.iter_mut() {
            // Look up the previous output
            match zebra.get_prev_output(&input.txid, input.vout) {
                Ok((value, address)) => {
                    input.value = Some(value);
                    input.address = address;
                }
                Err(_e) => {
                    // Previous output not found (might be from before our indexed range)
                    // This is normal during partial backfills
                }
            }
        }

        // Recalculate transparent_value_in
        tx.transparent_value_in = tx.vin.iter()
            .filter_map(|v| v.value)
            .sum();

        // Calculate fee:
        // fee = transparent_in + shielded_value_balance - transparent_out
        // where shielded_value_balance = sapling_value_balance + orchard_value_balance
        // (positive value_balance means ZEC leaving shielded pool = more inputs)
        let shielded_value_balance = tx.sapling_value_balance + tx.orchard_value_balance;
        let fee = tx.transparent_value_in + shielded_value_balance - tx.transparent_value_out;

        // Fee should always be positive (or zero for edge cases)
        tx.fee = if fee >= 0 { Some(fee) } else { None };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_encoding_mainnet() {
        let hash = hex::decode("0000000000000000000000000000000000000000").unwrap();
        let (p2pkh, p2sh) = TransactionParser::addr_prefixes(Network::Mainnet);
        assert!(TransactionParser::encode_address(&p2pkh, &hash).starts_with("t1"));
        assert!(TransactionParser::encode_address(&p2sh, &hash).starts_with("t3"));
    }

    #[test]
    fn test_address_encoding_testnet() {
        let hash = hex::decode("0000000000000000000000000000000000000000").unwrap();
        let (p2pkh, p2sh) = TransactionParser::addr_prefixes(Network::Testnet);
        assert!(TransactionParser::encode_address(&p2pkh, &hash).starts_with("tm"));
        assert!(TransactionParser::encode_address(&p2sh, &hash).starts_with("t2"));
    }
}
