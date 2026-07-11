-- Migration 005: Cross-chain swap tables for NEAR Intents integration
-- Stores historical swap data and daily amount stats for privacy recommendations

BEGIN;

-- Main table: every cross-chain swap involving ZEC
CREATE TABLE IF NOT EXISTS cross_chain_swaps (
  id SERIAL PRIMARY KEY,
  deposit_address TEXT NOT NULL UNIQUE,
  direction TEXT NOT NULL,              -- 'inflow' (other→ZEC) or 'outflow' (ZEC→other)
  status TEXT NOT NULL,                 -- SUCCESS, FAILED, PROCESSING, etc.
  source_chain TEXT,
  source_token TEXT,
  source_amount NUMERIC,
  source_amount_usd NUMERIC,
  source_tx_hashes TEXT[],
  dest_chain TEXT,
  dest_token TEXT,
  dest_amount NUMERIC,
  dest_amount_usd NUMERIC,
  dest_tx_hashes TEXT[],
  zec_txid TEXT,                        -- matched to transactions.txid
  zec_address TEXT,
  near_tx_hashes TEXT[],
  senders TEXT[],
  recipient TEXT,
  matched BOOLEAN DEFAULT FALSE,        -- whether zec_txid was found in transactions table
  match_attempts INTEGER DEFAULT 0,     -- retry counter for unmatched swaps
  swap_created_at TIMESTAMPTZ,
  indexed_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ccs_zec_txid ON cross_chain_swaps(zec_txid) WHERE zec_txid IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ccs_zec_address ON cross_chain_swaps(zec_address) WHERE zec_address IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ccs_created_at ON cross_chain_swaps(swap_created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ccs_direction ON cross_chain_swaps(direction);
CREATE INDEX IF NOT EXISTS idx_ccs_unmatched ON cross_chain_swaps(matched, match_attempts) WHERE matched = false;

-- Daily aggregated swap amounts for privacy-recommended amounts
CREATE TABLE IF NOT EXISTS swap_amount_stats_daily (
  date DATE NOT NULL,
  source_chain TEXT NOT NULL,
  source_token TEXT NOT NULL,
  amount_bucket NUMERIC NOT NULL,
  swap_count INTEGER NOT NULL,
  total_volume_usd NUMERIC,
  PRIMARY KEY (date, source_chain, source_token, amount_bucket)
);

CREATE INDEX IF NOT EXISTS idx_sasd_chain_token ON swap_amount_stats_daily(source_chain, source_token, date DESC);

-- Sync state tracker (one row per sync job)
CREATE TABLE IF NOT EXISTS sync_state (
  job_name TEXT PRIMARY KEY,
  last_sync_timestamp TIMESTAMPTZ,
  last_sync_count INTEGER DEFAULT 0,
  updated_at TIMESTAMPTZ DEFAULT NOW()
);

INSERT INTO sync_state (job_name, last_sync_timestamp)
VALUES ('crosschain_swaps', '2024-01-01T00:00:00Z')
ON CONFLICT (job_name) DO NOTHING;

COMMIT;
