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

        let lock_time_raw = tx.raw_lock_time();

        let (version, expiry_height_raw): (i32, Option<u32>) = match &tx {
            V1 { .. } => (1, None),
            V2 { .. } => (2, None),
            V3 { expiry_height, .. } => (3, Some(expiry_height.0)),
            V4 { expiry_height, .. } => (4, Some(expiry_height.0)),
            V5 { expiry_height, .. } => (5, Some(expiry_height.0)),
            V6 { expiry_height, .. } => (6, Some(expiry_height.0)),
        };

        // Get transparent inputs/outputs
        let inputs = tx.inputs();
        let outputs = tx.outputs();

        // Parse transparent inputs
        let mut vin: Vec<TransparentInput> = Vec::new();
        let transparent_value_in: i64 = 0;
        let mut is_coinbase = false;

        for input in inputs.iter() {
            use zebra_chain::transparent::Input;
            match input {
                Input::Coinbase { data, .. } => {
                    is_coinbase = true;
                    vin.push(TransparentInput {
                        txid: "0".repeat(64),
                        vout: 0xFFFFFFFF,
                        address: None,
                        value: None,
                        is_coinbase: true,
                        script_sig: Some(hex::encode(data)),
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
            transparent_value_out = transparent_value_out
                .checked_add(value_zat)
                .ok_or_else(|| format!("Overflow in transparent_value_out at output {}", n))?;

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
            // NU6.3 v6: Sapling is V5-shaped; the v6 Orchard bundle is ShieldedDataV6,
            // which exposes the underlying Orchard ShieldedData via .data(). Ironwood
            // is counted separately below.
            V6 { sapling_shielded_data, orchard_shielded_data, .. } => {
                let (spends, outputs) = sapling_shielded_data.as_ref()
                    .map(|d| (d.spends().count(), d.outputs().count()))
                    .unwrap_or((0, 0));
                let actions = orchard_shielded_data.as_ref()
                    .map(|d| d.data().actions.len())
                    .unwrap_or(0);
                (0, spends as u16, outputs as u16, actions as u16)
            }
        };

        // NU6.3 Ironwood actions (v6 only). ironwood::ShieldedData wraps an Orchard
        // v6 bundle; reach the Orchard actions through .data().
        let ironwood_actions: u16 = match &tx {
            V6 { ironwood_shielded_data, .. } => {
                ironwood_shielded_data.as_ref()
                    .map(|d| d.data().actions.len())
                    .unwrap_or(0) as u16
            }
            _ => 0,
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
            V6 { sapling_shielded_data, .. } => {
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
            V6 { orchard_shielded_data, .. } => {
                orchard_shielded_data.as_ref()
                    .map(|d| i64::from(d.data().value_balance))
                    .unwrap_or(0)
            }
            _ => 0,
        };

        // NU6.3 Ironwood value balance (v6 only). Negative = into Ironwood,
        // positive = out of Ironwood — same sign convention as Orchard.
        let ironwood_value_balance: i64 = match &tx {
            V6 { ironwood_shielded_data, .. } => {
                ironwood_shielded_data.as_ref()
                    .map(|d| i64::from(d.data().value_balance))
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
            ironwood_actions,
            sapling_value_balance,
            orchard_value_balance,
            ironwood_value_balance,
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
    use zebra_chain::transaction::LockTime;
    use zebra_chain::block::Height;
    use zebra_chain::serialization::ZcashSerialize;

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

    fn lock_time_to_u32(lt: LockTime) -> u32 {
        let mut buf = Vec::new();
        lt.zcash_serialize(&mut buf).unwrap();
        u32::from_le_bytes(buf.try_into().unwrap())
    }

    #[test]
    fn lock_time_unlocked_is_zero() {
        let lt = LockTime::unlocked();
        assert_eq!(lock_time_to_u32(lt), 0);
    }

    #[test]
    fn lock_time_height_preserves_value() {
        let lt = LockTime::Height(Height(400_000));
        assert_eq!(lock_time_to_u32(lt), 400_000);
    }

    #[test]
    fn lock_time_high_height_below_threshold() {
        let lt = LockTime::Height(Height(499_999_999));
        assert_eq!(lock_time_to_u32(lt), 499_999_999);
    }

    #[test]
    fn lock_time_time_above_threshold() {
        let lt = LockTime::min_lock_time_timestamp();
        assert_eq!(lock_time_to_u32(lt), 500_000_000);
    }
}
