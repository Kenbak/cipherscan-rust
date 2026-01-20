//! Transaction data model

use serde::{Serialize, Deserialize};

/// Zcash transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub txid: String,
    pub block_height: u32,
    pub block_hash: String,
    pub version: i32,
    pub lock_time: u32,
    pub expiry_height: Option<u32>,
    pub size: u32,

    // Transparent
    pub vin_count: u16,
    pub vout_count: u16,
    pub transparent_value_in: i64,
    pub transparent_value_out: i64,

    // Shielded counts
    pub joinsplit_count: u16,      // Sprout
    pub sapling_spends: u16,
    pub sapling_outputs: u16,
    pub orchard_actions: u16,

    // Value balances (negative = into shielded, positive = out of shielded)
    pub sapling_value_balance: i64,
    pub orchard_value_balance: i64,

    // Computed
    pub fee: Option<i64>,

    // Detailed data (optional, for API responses)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vin: Vec<TransparentInput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vout: Vec<TransparentOutput>,
}

impl Transaction {
    /// Check if this is a coinbase transaction
    pub fn is_coinbase(&self) -> bool {
        self.vin_count == 1 && self.vin.first().map(|v| v.is_coinbase).unwrap_or(false)
    }

    /// Check if transaction has shielded activity
    pub fn has_shielded(&self) -> bool {
        self.joinsplit_count > 0
            || self.sapling_spends > 0
            || self.sapling_outputs > 0
            || self.orchard_actions > 0
    }

    /// Check if this is a shielding transaction (transparent → shielded)
    pub fn is_shielding(&self) -> bool {
        self.vin_count > 0
            && (self.sapling_value_balance < 0 || self.orchard_value_balance < 0)
    }

    /// Check if this is a deshielding transaction (shielded → transparent)
    pub fn is_deshielding(&self) -> bool {
        self.vout_count > 0
            && (self.sapling_value_balance > 0 || self.orchard_value_balance > 0)
    }

    /// Check if this is fully shielded (no transparent involvement)
    pub fn is_fully_shielded(&self) -> bool {
        self.vin_count == 0
            && self.vout_count == 0
            && self.has_shielded()
            && !self.is_coinbase()
    }

    /// Get the dominant pool for shielded activity
    pub fn dominant_pool(&self) -> Option<&'static str> {
        if self.orchard_actions > 0 {
            Some("orchard")
        } else if self.sapling_spends > 0 || self.sapling_outputs > 0 {
            Some("sapling")
        } else if self.joinsplit_count > 0 {
            Some("sprout")
        } else {
            None
        }
    }
}

/// Transparent input (vin)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransparentInput {
    pub txid: String,       // Previous output txid
    pub vout: u32,          // Previous output index
    pub address: Option<String>,
    pub value: Option<i64>, // In zatoshis
    pub is_coinbase: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_sig: Option<String>,
}

/// Transparent output (vout)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransparentOutput {
    pub n: u32,             // Output index
    pub value: i64,         // In zatoshis
    pub address: Option<String>,
    pub script_type: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_pub_key: Option<String>,
}

impl TransparentOutput {
    /// Check if this is an OP_RETURN output
    pub fn is_op_return(&self) -> bool {
        self.script_type == "nulldata" || self.script_type == "op_return"
    }
}
