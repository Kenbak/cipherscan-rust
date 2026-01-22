---
title: PostgreSQL Write Patterns
impact: CRITICAL
impactDescription: Correct, efficient, idempotent database writes
tags: postgresql, sqlx, upsert, batch, writes
---

## PostgreSQL Write Patterns

Guidelines for writing to PostgreSQL from the Rust indexer.

### Connection

**Using sqlx:**
```rust
use sqlx::postgres::PgPoolOptions;

let pool = PgPoolOptions::new()
    .max_connections(20)
    .connect(&database_url)
    .await?;
```

### UPSERT Pattern (Critical)

**Always use ON CONFLICT for idempotency:**
```rust
// ✅ Good - Idempotent, can re-run safely
sqlx::query!(
    r#"
    INSERT INTO transactions (txid, block_height, fee, is_coinbase)
    VALUES ($1, $2, $3, $4)
    ON CONFLICT (txid) DO UPDATE SET
        block_height = EXCLUDED.block_height,
        fee = EXCLUDED.fee,
        is_coinbase = EXCLUDED.is_coinbase
    "#,
    txid, height, fee, is_coinbase
).execute(&pool).await?;

// ❌ Bad - Fails on duplicate
sqlx::query!(
    "INSERT INTO transactions (txid, block_height) VALUES ($1, $2)",
    txid, height
).execute(&pool).await?;
```

### Batch Inserts

**Use transactions for atomicity:**
```rust
let mut tx = pool.begin().await?;

for transaction in &transactions {
    sqlx::query!(
        "INSERT INTO transactions (...) VALUES (...) ON CONFLICT ...",
        // ...
    ).execute(&mut *tx).await?;
}

tx.commit().await?;
```

**Multi-value INSERT:**
```rust
// For bulk inserts without individual error handling
let values: Vec<String> = transactions
    .iter()
    .map(|t| format!("('{}', {}, {})", t.txid, t.height, t.fee))
    .collect();

let query = format!(
    "INSERT INTO transactions (txid, block_height, fee) VALUES {} 
     ON CONFLICT (txid) DO UPDATE SET 
         block_height = EXCLUDED.block_height,
         fee = EXCLUDED.fee",
    values.join(", ")
);

sqlx::query(&query).execute(&pool).await?;
```

### Checkpoint Management

**Separate checkpoints for backfill and live:**
```rust
// Backfill uses 'backfill_height'
sqlx::query!(
    r#"
    INSERT INTO indexer_state (key, value)
    VALUES ('backfill_height', $1)
    ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value
    "#,
    height.to_string()
).execute(&pool).await?;

// Live uses 'last_indexed_height'
sqlx::query!(
    r#"
    INSERT INTO indexer_state (key, value)
    VALUES ('last_indexed_height', $1)
    ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value
    "#,
    height.to_string()
).execute(&pool).await?;
```

**Read checkpoint:**
```rust
let checkpoint: Option<u32> = sqlx::query_scalar!(
    "SELECT value FROM indexer_state WHERE key = $1",
    "backfill_height"
)
.fetch_optional(&pool)
.await?
.flatten()
.and_then(|v| v.parse().ok());
```

### Table-Specific Patterns

**Blocks:**
```rust
sqlx::query!(
    r#"
    INSERT INTO blocks (
        height, hash, time, transaction_count, size, total_fees,
        version, difficulty, bits, nonce, merkle_root, previous_block_hash
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
    ON CONFLICT (height) DO UPDATE SET
        hash = EXCLUDED.hash,
        total_fees = EXCLUDED.total_fees,
        -- ... other fields
    "#,
    height, hash, time, tx_count, size, total_fees,
    version, difficulty, bits, nonce, merkle_root, prev_hash
).execute(&pool).await?;
```

**Shielded Flows:**
```rust
sqlx::query!(
    r#"
    INSERT INTO shielded_flows (
        txid, block_height, block_time, flow_type, pool, amount_zat
    ) VALUES ($1, $2, $3, $4, $5, $6)
    ON CONFLICT (txid, flow_type) DO UPDATE SET
        pool = EXCLUDED.pool,
        amount_zat = EXCLUDED.amount_zat
    "#,
    txid, height, time, flow_type, pool, amount
).execute(&pool).await?;
```

### Error Handling

```rust
match sqlx::query!(...).execute(&pool).await {
    Ok(_) => Ok(()),
    Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
        // Handle duplicate - usually OK with UPSERT
        tracing::debug!("Duplicate entry, already exists");
        Ok(())
    }
    Err(e) => Err(format!("Database error: {}", e)),
}
```

### Performance Tips

1. **Use batch inserts** - Fewer round trips
2. **Use transactions** - Atomic commits
3. **Index your queries** - Check EXPLAIN ANALYZE
4. **Pool connections** - Don't open/close per query
5. **Use UPSERT** - Avoids SELECT+INSERT race conditions
