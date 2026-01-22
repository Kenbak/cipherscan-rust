---
title: Rust Coding Patterns
impact: HIGH
impactDescription: Consistent, safe, performant Rust code
tags: rust, patterns, error-handling, async
---

## Rust Coding Patterns

Guidelines for writing Rust code in the CipherScan indexer.

### Error Handling

**Use Result with descriptive errors:**
```rust
// ✅ Good - Descriptive error with context
pub fn get_block_hash(&self, height: u32) -> Result<[u8; 32], String> {
    self.db
        .get_cf(&self.cf_hash_by_height, height.to_be_bytes())
        .map_err(|e| format!("RocksDB error at height {}: {}", height, e))?
        .ok_or_else(|| format!("Block not found at height {}", height))
}

// ❌ Bad - Panic on error
pub fn get_block_hash(&self, height: u32) -> [u8; 32] {
    self.db.get_cf(...).unwrap().unwrap()
}
```

**Use `?` operator for propagation:**
```rust
// ✅ Good
async fn index_block(&self, height: u32) -> Result<(), String> {
    let hash = self.zebra.get_block_hash(height)?;
    let header = self.zebra.get_block_header(height)?;
    self.postgres.insert_block(height, &hash, &header).await?;
    Ok(())
}

// ❌ Bad - Manual match
async fn index_block(&self, height: u32) -> Result<(), String> {
    let hash = match self.zebra.get_block_hash(height) {
        Ok(h) => h,
        Err(e) => return Err(e),
    };
    // ...
}
```

### Async Patterns

**Use tokio for async runtime:**
```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ...
}
```

**Await in loops carefully:**
```rust
// ✅ Good - Sequential when needed
for height in start..=end {
    self.index_block(height).await?;
}

// ✅ Good - Parallel when independent
let futures: Vec<_> = heights.iter()
    .map(|h| self.index_block(*h))
    .collect();
let results = futures::future::join_all(futures).await;
```

### Memory Efficiency

**Avoid unnecessary allocations:**
```rust
// ✅ Good - Reuse buffer
let mut buffer = Vec::with_capacity(1000);
for block in blocks {
    buffer.clear();
    // Use buffer
}

// ❌ Bad - Allocate each iteration
for block in blocks {
    let buffer = Vec::new();
    // Use buffer
}
```

**Use references when possible:**
```rust
// ✅ Good - Borrow
fn process_transaction(tx: &Transaction) -> Result<(), String> {
    // ...
}

// ❌ Bad - Unnecessary clone
fn process_transaction(tx: Transaction) -> Result<(), String> {
    // ...
}
```

### Logging

**Use tracing for structured logging:**
```rust
use tracing::{info, warn, error, debug};

// Levels:
// error! - Failures that need attention
// warn!  - Recoverable issues
// info!  - Important milestones
// debug! - Detailed debugging info

info!("Indexing block {}", height);
warn!("Block {} not found, retrying", height);
error!("Failed to insert block {}: {}", height, e);
debug!("Transaction {} has {} inputs", txid, inputs.len());
```

### Module Organization

```rust
// src/lib.rs or src/main.rs
mod config;
mod db;
mod indexer;
mod models;

// Re-export commonly used items
pub use config::Config;
pub use db::{ZebraState, PostgresWriter};
pub use models::{Transaction, ShieldedFlow};
```

### Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_calculation() {
        let tx = Transaction {
            transparent_value_in: 100000,
            transparent_value_out: 99000,
            ..Default::default()
        };
        assert_eq!(tx.fee, Some(1000));
    }

    #[tokio::test]
    async fn test_async_function() {
        // Async test
    }
}
```
