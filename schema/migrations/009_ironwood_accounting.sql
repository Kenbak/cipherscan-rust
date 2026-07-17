-- Migration 009: Ironwood indexer columns and accounting index
--
-- Adds Ironwood-specific columns to support NU6.3 pool tracking and creates
-- a partial covering index for efficient Ironwood reconciliation queries.
--
-- Run in two steps (CONCURRENTLY cannot be inside a transaction):
--
-- Step 1:
--   sudo -u postgres psql -v ON_ERROR_STOP=1 -d <dbname> -f 009_ironwood_accounting.sql
--
-- Step 2 (run separately):
--   sudo -u postgres psql -d <dbname> -c "
--     CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_transactions_ironwood_accounting
--       ON transactions (block_height)
--       INCLUDE (value_balance_ironwood, value_balance_orchard, is_coinbase)
--       WHERE has_ironwood = TRUE;
--   "
--
-- Step 3:
--   sudo -u postgres psql -d <dbname> -c "ANALYZE transactions;"

ALTER TABLE blocks
    ADD COLUMN IF NOT EXISTS final_orchard_root TEXT,
    ADD COLUMN IF NOT EXISTS final_ironwood_root TEXT,
    ADD COLUMN IF NOT EXISTS coinbase_hex TEXT;

ALTER TABLE transactions
    ADD COLUMN IF NOT EXISTS ironwood_actions INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS value_balance_ironwood BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS has_ironwood BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE orphaned_blocks
    ADD COLUMN IF NOT EXISTS final_sapling_root TEXT,
    ADD COLUMN IF NOT EXISTS final_orchard_root TEXT;

ALTER TABLE privacy_stats
    ADD COLUMN IF NOT EXISTS ironwood_pool_size BIGINT NOT NULL DEFAULT 0;

ALTER TABLE privacy_trends_daily
    ADD COLUMN IF NOT EXISTS ironwood_pool_size BIGINT NOT NULL DEFAULT 0;
