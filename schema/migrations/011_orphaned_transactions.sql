-- Migration 011: Orphaned transaction archival tables
--
-- Archives full transaction data from blocks that are reorged out of the
-- canonical chain. Enables debugging fork events without losing the tx data
-- that the indexer originally processed.
--
-- These tables are populated by the indexer's rollback_from_height() during
-- automatic reorg handling.
--
-- Run:
--   sudo -u postgres psql -v ON_ERROR_STOP=1 -d <dbname> -f 011_orphaned_transactions.sql

CREATE TABLE IF NOT EXISTS orphaned_transactions (
    txid text NOT NULL,
    block_height bigint NOT NULL,
    block_hash text NOT NULL,
    "timestamp" bigint,
    tx_index integer,
    version integer,
    locktime bigint,
    expiry_height integer,
    size integer,
    fee bigint DEFAULT 0,
    is_coinbase boolean DEFAULT false,
    vin_count integer DEFAULT 0,
    vout_count integer DEFAULT 0,
    total_input bigint DEFAULT 0,
    total_output bigint DEFAULT 0,
    has_sapling boolean DEFAULT false,
    has_orchard boolean DEFAULT false,
    has_sprout boolean DEFAULT false,
    has_ironwood boolean DEFAULT false,
    has_shielded_data boolean DEFAULT false,
    sapling_spend_count integer DEFAULT 0,
    sapling_output_count integer DEFAULT 0,
    orchard_actions integer DEFAULT 0,
    ironwood_actions integer DEFAULT 0,
    sprout_joinsplit_count integer DEFAULT 0,
    value_balance bigint DEFAULT 0,
    value_balance_sapling bigint DEFAULT 0,
    value_balance_orchard bigint DEFAULT 0,
    value_balance_ironwood bigint DEFAULT 0,
    flow_type text,
    privacy_score smallint,
    fork_event_id integer REFERENCES fork_events(id) ON DELETE SET NULL,
    first_indexed_at timestamp without time zone,
    archived_at timestamp without time zone DEFAULT now(),
    PRIMARY KEY (txid, block_hash)
);

CREATE INDEX IF NOT EXISTS idx_orphaned_tx_block ON orphaned_transactions (block_height DESC, tx_index);
CREATE INDEX IF NOT EXISTS idx_orphaned_tx_block_hash ON orphaned_transactions (block_hash);
CREATE INDEX IF NOT EXISTS idx_orphaned_tx_fork_event ON orphaned_transactions (fork_event_id);
CREATE INDEX IF NOT EXISTS idx_orphaned_tx_coinbase ON orphaned_transactions (block_height) WHERE is_coinbase = true;

CREATE TABLE IF NOT EXISTS orphaned_transaction_inputs (
    id bigserial PRIMARY KEY,
    txid text NOT NULL,
    block_hash text NOT NULL,
    vout_index integer,
    prev_txid text,
    prev_vout integer,
    address text,
    value bigint,
    coinbase text
);

CREATE INDEX IF NOT EXISTS idx_orphaned_txin_txid ON orphaned_transaction_inputs (txid, block_hash);

CREATE TABLE IF NOT EXISTS orphaned_transaction_outputs (
    id bigserial PRIMARY KEY,
    txid text NOT NULL,
    block_hash text NOT NULL,
    vout_index integer,
    value bigint,
    address text,
    script_type text
);

CREATE INDEX IF NOT EXISTS idx_orphaned_txout_txid ON orphaned_transaction_outputs (txid, block_hash);
