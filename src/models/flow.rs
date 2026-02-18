//! Shielded flow models
//!
//! Flows represent value moving between transparent and shielded pools.

use serde::{Serialize, Deserialize};
use crate::models::Transaction;

/// Type of shielded flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowType {
    /// Transparent to shielded
    Shield,
    /// Shielded to transparent
    Deshield,
    /// Between shielded pools (e.g., Sapling → Orchard)
    PoolMigration,
    /// Fully shielded (no transparent involvement)
    FullyShielded,
}

impl FlowType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlowType::Shield => "shield",
            FlowType::Deshield => "deshield",
            FlowType::PoolMigration => "pool_migration",
            FlowType::FullyShielded => "fully_shielded",
        }
    }
}

impl std::fmt::Display for FlowType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Shielded pool
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pool {
    Sprout,
    Sapling,
    Orchard,
}

impl Pool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Pool::Sprout => "sprout",
            Pool::Sapling => "sapling",
            Pool::Orchard => "orchard",
        }
    }
}

impl std::fmt::Display for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A shielded flow record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldedFlow {
    pub txid: String,
    pub flow_type: String,
    pub pool: String,
    pub amount: i64,  // In zatoshis (always positive)
    pub block_height: u32,
    pub transparent_addresses: Vec<String>,
}

impl ShieldedFlow {
    /// Analyze a transaction and extract flows
    /// Matches Node.js behavior EXACTLY:
    /// - Calculate NET total = sapling_value_balance + orchard_value_balance
    /// - If total > 0 → ONE deshield flow
    /// - If total < 0 → ONE shield flow
    /// - Pool = "mixed" if both pools have non-zero balance
    pub fn from_transaction(tx: &Transaction) -> Vec<ShieldedFlow> {
        let mut flows = Vec::new();

        // Skip coinbase
        if tx.is_coinbase() {
            return flows;
        }

        // Calculate NET total (exactly like Node.js)
        let total_value_balance = tx.sapling_value_balance + tx.orchard_value_balance;

        // Only create a flow if there's net movement
        if total_value_balance == 0 {
            return flows;
        }

        // No transparent inputs or outputs — check if this is a pool migration
        // or just a fully shielded tx where the value balance is the fee.
        // Pool migrations have opposing signs (one pool positive, the other negative).
        if tx.vin_count == 0 && tx.vout_count == 0 {
            let is_pool_migration =
                (tx.sapling_value_balance > 0 && tx.orchard_value_balance < 0)
                || (tx.orchard_value_balance > 0 && tx.sapling_value_balance < 0);
            if !is_pool_migration {
                return flows;
            }
        }

        // Collect transparent addresses for context
        let addresses: Vec<String> = tx.vin.iter()
            .filter_map(|v| v.address.clone())
            .chain(tx.vout.iter().filter_map(|v| v.address.clone()))
            .collect();

        // Determine flow type based on NET total (Node.js logic)
        let flow_type = if total_value_balance > 0 {
            FlowType::Deshield
        } else {
            FlowType::Shield
        };

        // Determine pool type (Node.js logic)
        // "mixed" if BOTH pools have non-zero balance (regardless of sign)
        let pool = if tx.sapling_value_balance != 0 && tx.orchard_value_balance != 0 {
            "mixed".to_string()
        } else if tx.orchard_value_balance != 0 {
            Pool::Orchard.to_string()
        } else {
            Pool::Sapling.to_string()
        };

        flows.push(ShieldedFlow {
            txid: tx.txid.clone(),
            flow_type: flow_type.to_string(),
            pool,
            amount: total_value_balance.abs(),  // Always positive
            block_height: tx.block_height,
            transparent_addresses: addresses,
        });

        flows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Transaction;

    #[test]
    fn test_flow_type_display() {
        assert_eq!(FlowType::Shield.as_str(), "shield");
        assert_eq!(FlowType::Deshield.as_str(), "deshield");
    }

    #[test]
    fn test_fully_shielded_tx_produces_no_flow() {
        let tx = Transaction {
            txid: "fully_shielded".to_string(),
            block_height: 3244398,
            block_hash: "hash".to_string(),
            version: 5,
            lock_time: 0,
            expiry_height: None,
            size: 200,
            vin_count: 0,
            vout_count: 0,
            transparent_value_in: 0,
            transparent_value_out: 0,
            joinsplit_count: 0,
            sapling_spends: 0,
            sapling_outputs: 0,
            orchard_actions: 2,
            sapling_value_balance: 0,
            orchard_value_balance: 10000, // fee only
            fee: Some(10000),
            vin: vec![],
            vout: vec![],
        };

        let flows = ShieldedFlow::from_transaction(&tx);
        assert!(flows.is_empty(), "Fully shielded tx should produce no flows");
    }

    #[test]
    fn test_pool_migration_still_produces_flow() {
        let tx = Transaction {
            txid: "pool_migration".to_string(),
            block_height: 3000000,
            block_hash: "hash".to_string(),
            version: 5,
            lock_time: 0,
            expiry_height: None,
            size: 500,
            vin_count: 0,
            vout_count: 0,
            transparent_value_in: 0,
            transparent_value_out: 0,
            joinsplit_count: 0,
            sapling_spends: 2,
            sapling_outputs: 0,
            orchard_actions: 2,
            sapling_value_balance: 5000000,    // 0.05 ZEC leaving Sapling
            orchard_value_balance: -4990000,   // ~0.05 ZEC entering Orchard (minus fee)
            fee: Some(10000),
            vin: vec![],
            vout: vec![],
        };

        let flows = ShieldedFlow::from_transaction(&tx);
        assert!(!flows.is_empty(), "Pool migration should still produce a flow");
    }
}
