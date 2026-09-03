//! Reusable CipherScan explorer indexing engine.
//!
//! The CLI remains in `main.rs`; this library exposes the concrete block
//! sources, parser, PostgreSQL sink, reorg coordinator, and chain-derived
//! models without requiring an operator to embed the CLI.

pub mod config;
pub mod db;
pub mod indexer;
pub mod models;
pub mod util;
