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

    #[test]
    fn test_flow_type_display() {
        assert_eq!(FlowType::Shield.as_str(), "shield");
        assert_eq!(FlowType::Deshield.as_str(), "deshield");
    }
}
