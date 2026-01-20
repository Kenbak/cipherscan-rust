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
    pub fn from_transaction(tx: &Transaction) -> Vec<ShieldedFlow> {
        let mut flows = Vec::new();
        
        // Skip coinbase
        if tx.is_coinbase() {
            return flows;
        }
        
        // Check for pool migration (shielded → shielded between pools)
        let is_pool_migration = tx.vin_count == 0 
            && tx.vout_count == 0
            && tx.has_shielded()
            && (tx.sapling_value_balance != 0 || tx.orchard_value_balance != 0);
        
        if is_pool_migration {
            // Sapling → Orchard migration
            if tx.sapling_value_balance > 0 && tx.orchard_value_balance < 0 {
                flows.push(ShieldedFlow {
                    txid: tx.txid.clone(),
                    flow_type: FlowType::PoolMigration.to_string(),
                    pool: "sapling_to_orchard".to_string(),
                    amount: tx.sapling_value_balance,
                    block_height: tx.block_height,
                    transparent_addresses: vec![],
                });
            }
            // Orchard → Sapling migration
            else if tx.orchard_value_balance > 0 && tx.sapling_value_balance < 0 {
                flows.push(ShieldedFlow {
                    txid: tx.txid.clone(),
                    flow_type: FlowType::PoolMigration.to_string(),
                    pool: "orchard_to_sapling".to_string(),
                    amount: tx.orchard_value_balance,
                    block_height: tx.block_height,
                    transparent_addresses: vec![],
                });
            }
            return flows;
        }
        
        // Fully shielded (no transparent, just shielded activity)
        if tx.is_fully_shielded() {
            let pool = tx.dominant_pool().unwrap_or("sapling");
            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::FullyShielded.to_string(),
                pool: pool.to_string(),
                amount: 0,  // Cannot determine amount for fully shielded
                block_height: tx.block_height,
                transparent_addresses: vec![],
            });
            return flows;
        }
        
        // Collect transparent addresses
        let addresses: Vec<String> = tx.vin.iter()
            .filter_map(|v| v.address.clone())
            .chain(tx.vout.iter().filter_map(|v| v.address.clone()))
            .collect();
        
        // Shielding (transparent → sapling/orchard)
        if tx.sapling_value_balance < 0 {
            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::Shield.to_string(),
                pool: Pool::Sapling.to_string(),
                amount: -tx.sapling_value_balance,
                block_height: tx.block_height,
                transparent_addresses: addresses.clone(),
            });
        }
        
        if tx.orchard_value_balance < 0 {
            flows.push(ShieldedFlow {
                txid: tx.txid.clone(),
                flow_type: FlowType::Shield.to_string(),
                pool: Pool::Orchard.to_string(),
                amount: -tx.orchard_value_balance,
                block_height: tx.block_height,
                transparent_addresses: addresses.clone(),
            });
        }
        
        // Deshielding (sapling/orchard → transparent)
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
