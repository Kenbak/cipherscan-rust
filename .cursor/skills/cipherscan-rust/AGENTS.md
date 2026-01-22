# CipherScan Rust Indexer - Agent Guidelines

> Compiled rules for AI agents working on the high-performance Rust blockchain indexer.

## Quick Reference

| Category | Impact | Key Points |
|----------|--------|------------|
| PostgreSQL Writes | Critical | Use UPSERT, batch inserts, separate checkpoints |
| Shielded Flows | Critical | Net balance, one flow per tx, mixed pool logic |
| RocksDB Access | High | Read-only, big-endian keys, zebra-chain parsing |
| Rust Patterns | High | Result types, async/await, tracing |
| Cross-Repo | High | Changes affect zcash-explorer API |

---

## Critical Rules

### 1. UPSERT for Idempotency (CRITICAL)
```rust
// ✅ ALWAYS use ON CONFLICT
sqlx::query!(
    "INSERT INTO transactions (...) VALUES (...)
     ON CONFLICT (txid) DO UPDATE SET ..."
).execute(&pool).await?;

// ❌ NEVER plain INSERT (fails on re-run)
sqlx::query!(
    "INSERT INTO transactions (...) VALUES (...)"
).execute(&pool).await?;
```

### 2. Shielded Flow Net Calculation (CRITICAL)
```rust
// Combine Sapling + Orchard into NET balance
let net = sapling_value_balance + orchard_value_balance;

// Negative = shield, Positive = deshield
let flow_type = if net < 0 { "shield" } else { "deshield" };

// Mixed pool only when BOTH are non-zero
let pool = if sapling != 0 && orchard != 0 { "mixed" } else { ... };
```

### 3. Values in Zatoshis (CRITICAL)
```rust
// Store as i64 (1 ZEC = 100,000,000 zatoshis)
let amount_zat: i64 = 100_000_000; // 1 ZEC

// Display
let zec = amount_zat as f64 / 100_000_000.0;
```

---

## Commands

```bash
# Build
cargo build --release

# Backfill (auto-resumes from checkpoint)
./target/release/cipherscan-indexer backfill

# Live mode (RPC-based)
./target/release/cipherscan-indexer live

# Validate against production
./target/release/cipherscan-indexer validate \
  --from-height 3200000 --to-height 3200100 \
  --prod-db "..." --test-db "..."
```

---

## Project Structure

```
src/
├── main.rs           # CLI (clap)
├── config.rs         # Configuration
├── db/
│   ├── rocks.rs      # RocksDB reader
│   ├── postgres.rs   # PostgreSQL writer
│   └── rpc.rs        # Zebra RPC client
├── indexer/
│   └── mod.rs        # Backfill + live logic
└── models/
    ├── transaction.rs
    └── flow.rs       # Shielded flow logic
```

---

## Database

### Checkpoints

| Key | Used By | Purpose |
|-----|---------|---------|
| `backfill_height` | Backfill | Last block indexed by backfill |
| `last_indexed_height` | Live | Last block indexed by live mode |

### Servers & Databases

| Network | Server IP | Database |
|---------|-----------|----------|
| Mainnet | 207.154.205.157 | zcash_explorer_mainnet |
| Testnet | 134.122.92.23 | zcash_explorer_testnet |
| Testing | localhost | zcash_test_rust |

SSH: `ssh root@207.154.205.157` (mainnet) or `ssh root@134.122.92.23` (testnet)

---

## RocksDB Column Families

| Column Family | Key | Value |
|---------------|-----|-------|
| block_header_by_height | height (BE u32) | header |
| hash_by_height | height (BE u32) | [u8; 32] |
| tx_loc_by_hash | tx hash | location |
| utxo_by_out_loc | out location | UTXO data |

**Zebra State Path:** `/root/.cache/zebra/state/v27/mainnet`

---

## Deployment

### Systemd Service

```bash
sudo systemctl start cipherscan-rust    # Live mode
sudo systemctl status cipherscan-rust
journalctl -u cipherscan-rust -f
```

### Backfill (Screen)

```bash
screen -S backfill
./target/release/cipherscan-indexer backfill
# Ctrl+A, D to detach
# screen -r backfill to reattach
```

---

## Cross-Repo Impact

Changes here affect `zcash-explorer`:
- Schema changes → Update API queries
- New fields → Update frontend display
- Flow logic changes → Verify data parity

**Always validate before deploying schema changes.**

---

*Generated from cipherscan-rust skill rules*
