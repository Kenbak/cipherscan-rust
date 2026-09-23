-- 026: Local-node tip observations and complete orphan metadata.
-- Observation times are collector receipt times, never inferred from headers or indexing.
-- Apply before the new indexer/API. Existing blocks intentionally remain unobserved.
BEGIN;
SET LOCAL lock_timeout = '2s';
SET LOCAL statement_timeout = '120s';
CREATE TABLE IF NOT EXISTS public.block_observations (
    hash text PRIMARY KEY CHECK (hash ~ '^[0-9a-f]{64}$'),
    height bigint NOT NULL CHECK (height >= 0),
    first_seen_at timestamptz NOT NULL,
    source text NOT NULL CHECK (source = 'local-node-rpc'),
    poll_interval_ms integer NOT NULL CHECK (poll_interval_ms > 0)
);
ALTER TABLE public.orphaned_blocks ADD COLUMN IF NOT EXISTS block_metadata jsonb;
ALTER TABLE public.orphaned_blocks ADD COLUMN IF NOT EXISTS final_ironwood_root text;
ALTER TABLE public.orphaned_blocks ADD COLUMN IF NOT EXISTS raw_hex text;
-- Match existing block readers/writers without granting access to PUBLIC.
DO $$
DECLARE r record;
BEGIN
  FOR r IN SELECT DISTINCT grantee FROM information_schema.role_table_grants
    WHERE table_schema='public' AND table_name='blocks' AND grantee <> 'PUBLIC'
  LOOP
    IF has_table_privilege(r.grantee,'public.blocks','SELECT') THEN
      EXECUTE format('GRANT SELECT ON public.block_observations TO %I', r.grantee);
    END IF;
    IF has_table_privilege(r.grantee,'public.blocks','INSERT') THEN
      EXECUTE format('GRANT INSERT ON public.block_observations TO %I', r.grantee);
    END IF;
  END LOOP;
END $$;
COMMIT;
