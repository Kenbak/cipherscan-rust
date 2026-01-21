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
    /// Matches Node.js behavior: ONE flow per (txid, flow_type) with pool="mixed" when both pools used
    /// DB constraint is UNIQUE(txid, flow_type), so we must combine pools
    pub fn from_transaction(tx: &Transaction) -> Vec<ShieldedFlow> {
        let mut flows = Vec::new();

        // Skip coinbase
        if tx.is_coinbase() {
            return flows;
        }

        // Note: We do NOT skip fully shielded transactions (vin_count=0, vout_count=0)
        // Node.js creates flows for all transactions with value_balance != 0
        // This ensures compatibility and captures fee payments from shielded funds

        // Collect transparent addresses for context (may be empty for fully shielded)
        let addresses: Vec<String> = tx.vin.iter()
            .filter_map(|v| v.address.clone())
            .chain(tx.vout.iter().filter_map(|v| v.address.clone()))
            .collect();

        // Check for shielding (value_balance < 0 = value entering shielded pool)
        let sapling_shield = if tx.sapling_value_balance < 0 { -tx.sapling_value_balance } else { 0 };
        let orchard_shield = if tx.orchard_value_balance < 0 { -tx.orchard_value_balance } else { 0 };
        let total_shield = sapling_shield + orchard_shield;

        if total_shield > 0 {
            // Determine pool type (match Node.js logic)
            let pool = if sapling_shield > 0 && orchard_shield > 0 {
                "mixed".to_string()
            } else if orchard_shield > 0 {
                Pool::Orchard.to_string()
            } else {
                Pool::Sapling.to_string()
            };

            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::Shield.to_string(),
                pool,
                amount: total_shield,
                block_height: tx.block_height,
                transparent_addresses: addresses.clone(),
            });
        }

        // Check for deshielding (value_balance > 0 = value leaving shielded pool)
        let sapling_deshield = if tx.sapling_value_balance > 0 { tx.sapling_value_balance } else { 0 };
        let orchard_deshield = if tx.orchard_value_balance > 0 { tx.orchard_value_balance } else { 0 };
        let total_deshield = sapling_deshield + orchard_deshield;

        if total_deshield > 0 {
            // Determine pool type (match Node.js logic)
            let pool = if sapling_deshield > 0 && orchard_deshield > 0 {
                "mixed".to_string()
            } else if orchard_deshield > 0 {
                Pool::Orchard.to_string()
            } else {
                Pool::Sapling.to_string()
            };

            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::Deshield.to_string(),
                pool,
                amount: total_deshield,
                block_height: tx.block_height,
                transparent_addresses: addresses,
            });
        }

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
