# CipherScan Rust Indexer

High-performance Zcash blockchain indexer written in Rust. Reads directly from Zebra's RocksDB state for maximum speed.

## Features

- **Direct RocksDB access**: No RPC overhead, reads Zebra's state directly
- **~100 blocks/sec**: 10-50x faster than the Node.js indexer
- **Full data parity**: Produces identical data to the Node.js indexer
- **PostgreSQL output**: Same schema, drop-in replacement
- **Validation mode**: Compare output against existing production data

## Requirements

- Rust 1.75+ (for async traits)
- Running Zebra node with synced state
- PostgreSQL 14+
- ~50GB disk space for the indexed database

## Installation

```bash
# Clone the repository
git clone https://github.com/Kenbak/cipherscan-rust.git
cd cipherscan-rust

# Build release binary
cargo build --release

# Binary will be at ./target/release/cipherscan-indexer
```

## Configuration

Create a `.env` file or set environment variables:

```bash
# Required: Path to Zebra's RocksDB state
ZEBRA_STATE_PATH=/root/.cache/zebra/state/v27/mainnet

# Required: PostgreSQL connection URL
DATABASE_URL=postgres://user:password@localhost/zcash_explorer

# Optional: Network (mainnet or testnet)
NETWORK=mainnet
```

## Usage

### Commands

```bash
# Show help
./target/release/cipherscan-indexer --help

# Analyze Zebra's database structure
./target/release/cipherscan-indexer analyze

# Show indexer status
./target/release/cipherscan-indexer status

# Backfill from genesis (or checkpoint) to current tip
./target/release/cipherscan-indexer backfill

# Backfill specific range
./target/release/cipherscan-indexer backfill --from 3000000 --to 3100000

# Follow chain tip in real-time
./target/release/cipherscan-indexer live

# Validate against production database
./target/release/cipherscan-indexer validate \
  --prod-db "postgres://user:pass@localhost/prod_db" \
  --test-db "postgres://user:pass@localhost/test_db" \
  --from-height 3200000 \
  --to-height 3200100
```

### Validation Mode

The `validate` command is essential before replacing the Node.js indexer:

1. Indexes a range of blocks into a test database
2. Compares every field with the production database
3. Reports any mismatches with detailed diffs
4. Benchmarks performance

```bash
# Validate 10,000 blocks
./target/release/cipherscan-indexer validate \
  --from-height 3190000 --to-height 3200000 \
  --prod-db "postgres://..." \
  --test-db "postgres://..."
```

Expected output:
```
🎉 VALIDATION PASSED! Rust indexer matches production data.

🚀 Performance:
   Rust: 102.3s for 10001 blocks (97.8 blk/s, 691.2 tx/s)

🔍 Data Comparison:
   Transactions: 71234/71234 matched (100.0%)
   Outputs:      129456/129456 matched (100.0%)
   Inputs:       101234/101234 matched (100.0%)
   Flows:        8923/8923 matched (100.0%)
```

## Database Schema

Uses the same PostgreSQL schema as the Node.js indexer. Key tables:

| Table | Description |
|-------|-------------|
| `blocks` | Block headers with all fields (version, difficulty, nonce, etc.) |
| `transactions` | Transaction data with fees, value balances, counts |
| `transaction_inputs` | Transparent inputs with resolved addresses and values |
| `transaction_outputs` | Transparent outputs |
| `shielded_flows` | Shield/deshield flows for privacy analysis |
| `indexer_state` | Checkpoint for resumable indexing |

### Schema Setup

```bash
# Apply the schema to a new database
psql -U user -d zcash_test_rust -f schema/postgres.sql

# Or use the migration for existing databases
psql -U user -d zcash_explorer -f schema/migrations/001_rust_indexer_support.sql
```

## Architecture

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Zebra Node    │────▶│  Rust Indexer   │────▶│   PostgreSQL    │
│   (RocksDB)     │     │                 │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                       │
        │                       ├── Block parsing (zebra-chain)
        │                       ├── Transaction parsing
        │                       ├── Input resolution (UTXO lookup)
        │                       ├── Flow generation
        │                       └── Batch PostgreSQL writes
        │
        └── Direct column family access:
            ├── block_header_by_height
            ├── hash_by_height
            ├── tx_loc_by_hash
            ├── utxo_by_out_loc
            └── height_by_hash
```

## Performance

Benchmarks on AMD EPYC (8 cores, 32GB RAM, NVMe SSD):

| Metric | Node.js Indexer | Rust Indexer | Speedup |
|--------|-----------------|--------------|---------|
| Blocks/sec | ~2-5 | ~100 | **20-50x** |
| Transactions/sec | ~20-50 | ~700 | **15-35x** |
| Memory usage | ~500MB | ~100MB | **5x less** |

## Data Indexed

### Blocks
- Height, hash, timestamp
- Version, difficulty, bits, nonce
- Merkle root, final Sapling root
- Previous block hash
- Size, transaction count
- Total fees, miner address

### Transactions
- txid, block height, version, locktime
- Size, fee (calculated)
- Input/output counts
- Value balances (Sapling, Orchard)
- Shielded component counts
- is_coinbase flag

### Transparent Inputs
- Previous output reference (txid, vout)
- Resolved address and value (via UTXO lookup)
- Script signature

### Transparent Outputs
- Address, value
- Script type, script hex

### Shielded Flows
- Flow type: shield or deshield
- Pool: sapling, orchard, or mixed
- Amount (net total)
- Associated transparent addresses

## Troubleshooting

### "RocksDB: Invalid argument"
Zebra is still running. Stop Zebra or wait for it to release the lock.

### "column family not found"
The Zebra state version may be different. Check `ZEBRA_STATE_PATH` points to the correct version (e.g., `v27`).

### Slow performance
- Ensure NVMe SSD for both RocksDB and PostgreSQL
- Increase PostgreSQL `shared_buffers` and `work_mem`
- Use `--release` build (debug builds are 10x slower)

## Development

```bash
# Run tests
cargo test

# Check for errors without building
cargo check

# Format code
cargo fmt

# Run with debug logging
RUST_LOG=debug cargo run -- status
```

## License

MIT License - see LICENSE file.

## Related Projects

- [Zebra](https://github.com/ZcashFoundation/zebra) - Zcash node implementation
- [CipherScan Explorer](https://github.com/Kenbak/zcash-explorer) - Frontend and API
