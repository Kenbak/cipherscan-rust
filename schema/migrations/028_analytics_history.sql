-- Additive analytics only; no changes to indexer-owned blocks or transactions.
-- Run in a short transaction with a lock_timeout. Daily checkpoints allow
-- bounded replay and recent-day repair without rescanning the live UTXO set.
CREATE TABLE IF NOT EXISTS analytics_history_daily (
 date date PRIMARY KEY,
 anchor_height bigint NOT NULL,
 anchor_hash text NOT NULL,
 method text NOT NULL CHECK (method = 'transparent-utxo-day-close-v1'),
 cohorts jsonb NOT NULL,
 transparent_supply_zat bigint NOT NULL CHECK (transparent_supply_zat >= 0),
 transparent_realized_cap_usd numeric NOT NULL CHECK (transparent_realized_cap_usd >= 0),
 spent_value_zat bigint NOT NULL CHECK (spent_value_zat >= 0),
 spent_creation_value_usd numeric NOT NULL CHECK (spent_creation_value_usd >= 0),
 sopr numeric,
 fees jsonb NOT NULL,
 computed_at timestamptz NOT NULL DEFAULT now()
);
COMMENT ON TABLE analytics_history_daily IS 'Completed UTC-day transparent output replay; USD daily-price model, not owner purchase prices. SOPR excludes shielded notes. Cohorts contain only public aggregate values/counts.';
DO $$ BEGIN
 EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON analytics_history_daily TO %I',
 (SELECT tableowner FROM pg_tables WHERE schemaname='public' AND tablename='blocks'));
END $$;
