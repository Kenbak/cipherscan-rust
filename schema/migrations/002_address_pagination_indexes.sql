-- Migration: Add indexes for optimized address pagination
-- Run on: zcash_explorer_mainnet, zcash_explorer_testnet
-- 
-- This migration adds composite indexes to dramatically speed up
-- address transaction queries with cursor-based pagination.
--
-- Expected improvements:
-- - Address with 10,000+ txs: 30s+ → <100ms
-- - Cursor pagination: O(1) instead of O(n) with OFFSET

-- Index 1: Composite index for finding transactions by address efficiently
-- Used in: WHERE txid IN (SELECT txid FROM transaction_outputs WHERE address = $1)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tx_outputs_addr_created
ON transaction_outputs(address, created_at DESC);

CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_tx_inputs_addr_created
ON transaction_inputs(address, created_at DESC);

-- Index 2: Covering index for transaction lookups with ordering
-- This helps ORDER BY block_height DESC, tx_index DESC be instant
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_transactions_height_index_txid
ON transactions(block_height DESC, tx_index DESC, txid);

-- Index 3: Composite for block_time ordering (alternative)
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_transactions_blocktime_txid
ON transactions(block_time DESC, txid);

-- Analyze tables to update statistics
ANALYZE transaction_inputs;
ANALYZE transaction_outputs;
ANALYZE transactions;

-- Verify indexes were created
SELECT indexname, indexdef 
FROM pg_indexes 
WHERE tablename IN ('transaction_inputs', 'transaction_outputs', 'transactions')
AND indexname LIKE 'idx_%addr%' OR indexname LIKE 'idx_transactions_height%' OR indexname LIKE 'idx_transactions_blocktime%';
