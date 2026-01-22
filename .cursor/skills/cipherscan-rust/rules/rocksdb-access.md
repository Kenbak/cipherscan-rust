---
title: RocksDB Access Patterns
impact: CRITICAL
impactDescription: Correct reading of Zebra's blockchain state
tags: rocksdb, zebra, blockchain, state
---

## RocksDB Access Patterns

Guidelines for reading Zebra's RocksDB state.

### Column Families

Zebra stores data in specific column families:

| Column Family | Key | Value | Usage |
|---------------|-----|-------|-------|
| `block_header_by_height` | height (BE u32) | serialized header | Block headers |
| `hash_by_height` | height (BE u32) | [u8; 32] | Block hash at height |
| `height_by_hash` | [u8; 32] | height (BE u32) | Height for hash |
| `tx_loc_by_hash` | tx hash | location | Transaction location |
| `utxo_by_out_loc` | out location | UTXO data | UTXO lookup |

### Opening RocksDB

**Read-only mode for backfill:**
```rust
use rocksdb::{DB, Options, ColumnFamilyDescriptor};

let cf_names = DB::list_cf(&Options::default(), path)?;
let cf_descriptors: Vec<_> = cf_names
    .iter()
    .map(|name| ColumnFamilyDescriptor::new(name, Options::default()))
    .collect();

let db = DB::open_cf_descriptors_read_only(
    &Options::default(),
    path,
    cf_descriptors,
    false, // error_if_wal_exists
)?;
```

**Secondary mode for live (experimental):**
```rust
let secondary_path = format!("{}_secondary", path);
let db = DB::open_cf_as_secondary(
    &Options::default(),
    path,
    &secondary_path,
    cf_descriptors,
)?;

// Catch up with primary
db.try_catch_up_with_primary()?;
```

### Reading Data

**Get block hash at height:**
```rust
fn get_block_hash(&self, height: u32) -> Result<[u8; 32], String> {
    let key = height.to_be_bytes(); // Big-endian!
    
    let value = self.db
        .get_cf(&self.cf_hash_by_height, key)
        .map_err(|e| format!("RocksDB error: {}", e))?
        .ok_or_else(|| format!("Block not found at height {}", height))?;
    
    let hash: [u8; 32] = value
        .try_into()
        .map_err(|_| "Invalid hash length")?;
    
    Ok(hash)
}
```

**Get tip height:**
```rust
fn get_tip_height(&self) -> Result<u32, String> {
    let mut iter = self.db.raw_iterator_cf(&self.cf_hash_by_height);
    iter.seek_to_last();
    
    if iter.valid() {
        let key = iter.key().ok_or("No key")?;
        let height = u32::from_be_bytes(key.try_into().map_err(|_| "Invalid key")?);
        Ok(height)
    } else {
        Err("No blocks found".to_string())
    }
}
```

### Parsing with zebra-chain

**Block header:**
```rust
use zebra_chain::block::Header;
use zebra_chain::serialization::ZcashDeserializeInto;

fn parse_header(raw: &[u8]) -> Result<Header, String> {
    raw.zcash_deserialize_into()
        .map_err(|e| format!("Header parse error: {:?}", e))
}
```

**Transaction:**
```rust
use zebra_chain::transaction::Transaction as ZebraTransaction;

fn parse_transaction(raw: &[u8]) -> Result<ZebraTransaction, String> {
    let mut cursor = std::io::Cursor::new(raw);
    ZebraTransaction::zcash_deserialize(&mut cursor)
        .map_err(|e| format!("Transaction parse error: {:?}", e))
}
```

### UTXO Lookup

**Resolve input address and value:**
```rust
fn resolve_input(&self, prev_txid: &str, prev_vout: u32) -> Option<(String, i64)> {
    // Build output location key
    let key = build_out_loc_key(prev_txid, prev_vout);
    
    // Lookup in UTXO column family
    if let Ok(Some(value)) = self.db.get_cf(&self.cf_utxo, key) {
        let (address, amount) = parse_utxo(&value)?;
        return Some((address, amount));
    }
    
    None
}
```

### Important Notes

1. **Keys are big-endian** - Use `to_be_bytes()` for heights
2. **Hash byte order** - Zebra uses internal order, reverse for display
3. **Read-only is safer** - Don't write to Zebra's state
4. **Close properly** - Release DB handle when done
5. **Zebra must be stopped** - Or use secondary mode
