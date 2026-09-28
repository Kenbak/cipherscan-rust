-- Exact local-node tip accounting. This is sampled history, not historical backfill.
-- Keep orphan observations; readers MUST join canonical blocks by BOTH height and hash.
-- Deploy before the observing indexer. No production activation date is encoded here.
BEGIN;
SET LOCAL lock_timeout = '2s';
SET LOCAL statement_timeout = '120s';
CREATE TABLE IF NOT EXISTS public.node_accounting_observations (
    hash text PRIMARY KEY CHECK (hash ~ '^[0-9a-f]{64}$'),
    height bigint NOT NULL CHECK (height >= 0),
    observed_at timestamptz NOT NULL,
    chain text,
    nsm_balance_zat bigint,
    circulating_supply_zat bigint CHECK (circulating_supply_zat BETWEEN 0 AND 2100000000000000),
    pool_balances_zat jsonb NOT NULL,
    subsidy jsonb,
    upgrades jsonb,
    source text NOT NULL CHECK (source = 'getblockchaininfo'),
    poll_interval_ms integer NOT NULL CHECK (poll_interval_ms > 0)
);
COMMENT ON TABLE public.node_accounting_observations IS
'Hash-bound local tip samples. Null means unreported. NSM is a signed reserve, not circulating supply. No interpolation or inferred reissuance.';
DO $$
DECLARE r record;
BEGIN
  FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants
    WHERE table_schema='public' AND table_name='blocks' AND grantee <> 'PUBLIC'
  LOOP
    IF has_table_privilege(r.grantee,'public.blocks','SELECT') THEN
      EXECUTE format('GRANT SELECT ON public.node_accounting_observations TO %I', r.grantee);
    END IF;
    IF has_table_privilege(r.grantee,'public.blocks','INSERT') THEN
      EXECUTE format('GRANT INSERT ON public.node_accounting_observations TO %I', r.grantee);
    END IF;
  END LOOP;
END $$;
COMMIT;
