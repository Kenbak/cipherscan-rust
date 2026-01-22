---
title: Shielded Flow Logic
impact: CRITICAL
impactDescription: Must match Node.js indexer for data parity
tags: shielded, flows, sapling, orchard, privacy
---

## Shielded Flow Logic

Guidelines for generating shielded flows that match the Node.js indexer exactly.

### Flow Types

| Type | Direction | Value Balance | Description |
|------|-----------|---------------|-------------|
| `shield` | t→z | Negative | Transparent to shielded |
| `deshield` | z→t | Positive | Shielded to transparent |

### Pool Types

| Pool | When Used |
|------|-----------|
| `sapling` | Only Sapling pool involved |
| `orchard` | Only Orchard pool involved |
| `mixed` | Both Sapling AND Orchard in same tx |
| `sprout` | Legacy Sprout (rare) |

### Core Logic

**In `src/models/flow.rs`:**
```rust
impl ShieldedFlow {
    pub fn from_transaction(tx: &Transaction) -> Vec<ShieldedFlow> {
        let mut flows = Vec::new();
        
        // Calculate NET balance (combine Sapling + Orchard)
        let total_value_balance = 
            tx.sapling_value_balance + tx.orchard_value_balance;
        
        // No flow if balance is zero
        if total_value_balance == 0 {
            return flows;
        }
        
        // Determine pool type
        let pool = if tx.sapling_value_balance != 0 && tx.orchard_value_balance != 0 {
            "mixed"  // Both pools used
        } else if tx.sapling_value_balance != 0 {
            "sapling"
        } else {
            "orchard"
        };
        
        // Determine flow type and amount
        let (flow_type, amount) = if total_value_balance < 0 {
            ("shield", total_value_balance.abs())
        } else {
            ("deshield", total_value_balance)
        };
        
        flows.push(ShieldedFlow {
            txid: tx.txid.clone(),
            flow_type: flow_type.to_string(),
            pool: pool.to_string(),
            amount_zat: amount,
        });
        
        flows
    }
}
```

### Critical Rules

1. **ONE flow per transaction per type**
   - Unique constraint: `(txid, flow_type)`
   - Don't create separate flows for Sapling and Orchard
   - Combine into single net flow

2. **Net calculation**
   - `total = sapling_value_balance + orchard_value_balance`
   - This is the NET movement, not gross

3. **Sign convention**
   - Negative = INTO shielded pool = shield
   - Positive = OUT OF shielded pool = deshield

4. **Include fully shielded transactions**
   - Even if `vin_count == 0` and `vout_count == 0`
   - These are z→z transfers that may still have net value changes

5. **Pool = "mixed" only when both are non-zero**
   ```rust
   let pool = if sapling != 0 && orchard != 0 { "mixed" } else { ... };
   ```

### Value Balance Convention

From Zebra/Zcash:
```
sapling_value_balance:
  - Negative: ZEC going INTO Sapling pool (shielding)
  - Positive: ZEC coming OUT of Sapling pool (deshielding)

orchard_value_balance:
  - Same convention as Sapling
```

### Database Constraints

```sql
-- Only these flow types allowed
CHECK (flow_type = ANY (ARRAY['shield', 'deshield']))

-- Only these pools allowed  
CHECK (pool = ANY (ARRAY['sapling', 'orchard', 'sprout', 'mixed']))

-- One flow per type per transaction
UNIQUE (txid, flow_type)
```

### Testing

**Validate against production:**
```bash
./target/release/cipherscan-indexer validate \
  --from-height 3200000 --to-height 3200100 \
  --prod-db "..." --test-db "..."
```

Check for:
- Flow count matches
- Pool types match
- Amounts match
- No missing or extra flows
