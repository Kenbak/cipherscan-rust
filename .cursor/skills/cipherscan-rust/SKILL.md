---
name: cipherscan-rust
description: CipherScan Rust indexer project rules. High-performance Zcash blockchain indexer.
metadata:
  author: cipherscan
  version: "1.0.0"
---

# CipherScan Rust Indexer Rules

Project-specific guidelines for the high-performance Rust blockchain indexer.

## When to Use

These rules apply to ALL work on the Rust indexer:
- RocksDB reading from Zebra
- PostgreSQL writing
- Transaction and block parsing
- Shielded flow generation

## Categories

| Category | Priority | Description |
|----------|----------|-------------|
| Database Write | Critical | PostgreSQL inserts, UPSERTs |
| Zcash Parsing | Critical | Transaction and block parsing |
| RocksDB Access | High | Reading Zebra's state |
| Cross-Repo | High | Impact on zcash-explorer |
| Deployment | Medium | Systemd, screen, backfill |

## Related Projects

- **zcash-explorer**: Frontend and API (shares same database)
- Both projects write/read the same PostgreSQL database
