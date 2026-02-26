-- Migration 004: Create address_transactions denormalized table
--
-- Maps addresses directly to their transactions for instant pagination.
-- Eliminates the expensive UNION + JOIN query on the address page.
--
-- Run: psql -d <database> -f 004_address_transactions_table.sql

CREATE TABLE IF NOT EXISTS address_transactions (
  address TEXT NOT NULL,
  txid TEXT NOT NULL,
  block_height INT NOT NULL,
  tx_index INT NOT NULL DEFAULT 0,
  block_time BIGINT,
  is_input BOOLEAN DEFAULT false,
  is_output BOOLEAN DEFAULT false,
  value_in BIGINT DEFAULT 0,
  value_out BIGINT DEFAULT 0,
  PRIMARY KEY (address, block_height, tx_index, txid)
);

-- Main lookup index for paginated address queries
CREATE INDEX IF NOT EXISTS idx_addr_tx_lookup
  ON address_transactions (address, block_height DESC, tx_index DESC);

-- Reverse lookup: find all addresses in a transaction
CREATE INDEX IF NOT EXISTS idx_addr_tx_by_txid
  ON address_transactions (txid);

ANALYZE address_transactions;
