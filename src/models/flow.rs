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
    /// Only generates 'shield' and 'deshield' flows (DB constraint compatible)
    /// Pool migrations and fully shielded are detectable from transaction fields
    pub fn from_transaction(tx: &Transaction) -> Vec<ShieldedFlow> {
        let mut flows = Vec::new();

        // Skip coinbase
        if tx.is_coinbase() {
            return flows;
        }

        // Skip pool migrations and fully shielded - no transparent involvement
        // These don't belong in shielded_flows (no transparent addresses to track)
        if tx.vin_count == 0 && tx.vout_count == 0 {
            return flows;
        }

        // Collect transparent addresses for context
        let addresses: Vec<String> = tx.vin.iter()
            .filter_map(|v| v.address.clone())
            .chain(tx.vout.iter().filter_map(|v| v.address.clone()))
            .collect();

        // Shielding (transparent → sapling/orchard)
        // value_balance < 0 means value is entering the shielded pool
        if tx.sapling_value_balance < 0 {
            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::Shield.to_string(),
                pool: Pool::Sapling.to_string(),
                amount: -tx.sapling_value_balance,  // Make positive
                block_height: tx.block_height,
                transparent_addresses: addresses.clone(),
            });
        }

        if tx.orchard_value_balance < 0 {
            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::Shield.to_string(),
                pool: Pool::Orchard.to_string(),
                amount: -tx.orchard_value_balance,  // Make positive
                block_height: tx.block_height,
                transparent_addresses: addresses.clone(),
            });
        }

        // Deshielding (sapling/orchard → transparent)
        // value_balance > 0 means value is leaving the shielded pool
        if tx.sapling_value_balance > 0 {
            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::Deshield.to_string(),
                pool: Pool::Sapling.to_string(),
                amount: tx.sapling_value_balance,
                block_height: tx.block_height,
                transparent_addresses: addresses.clone(),
            });
        }

        if tx.orchard_value_balance > 0 {
            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::Deshield.to_string(),
                pool: Pool::Orchard.to_string(),
                amount: tx.orchard_value_balance,
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
