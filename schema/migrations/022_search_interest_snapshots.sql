-- Migration 022: versioned Google Trends exports for valuation context.
-- API/job-owned analytics; no consensus/indexer write path changes.
BEGIN;
CREATE TABLE IF NOT EXISTS public.search_interest_snapshots (
  id text PRIMARY KEY CHECK (id ~ '^[a-f0-9]{64}$'),
  provider text NOT NULL CHECK (provider = 'google_trends'),
  query text NOT NULL CHECK (query = 'zcash'),
  geography text NOT NULL CHECK (geography = 'worldwide'),
  resolution text NOT NULL CHECK (resolution = 'week'),
  category text NOT NULL CHECK (category = 'all'),
  search_property text NOT NULL CHECK (search_property = 'web'),
  acquisition text NOT NULL CHECK (acquisition = 'csv_import'),
  captured_at timestamptz NOT NULL,
  imported_at timestamptz NOT NULL DEFAULT now(),
  window_start date NOT NULL,
  window_end date NOT NULL CHECK (window_end >= window_start)
);
CREATE TABLE IF NOT EXISTS public.search_interest_points (
  snapshot_id text NOT NULL REFERENCES public.search_interest_snapshots(id) ON DELETE CASCADE,
  date date NOT NULL,
  value smallint CHECK (value BETWEEN 0 AND 100),
  below_one boolean NOT NULL DEFAULT false,
  partial boolean NOT NULL DEFAULT false,
  PRIMARY KEY (snapshot_id, date),
  CHECK ((below_one AND value IS NULL) OR (NOT below_one AND value IS NOT NULL))
);
COMMIT;

CREATE INDEX CONCURRENTLY IF NOT EXISTS search_interest_latest_idx
  ON public.search_interest_snapshots (window_end DESC, captured_at DESC);
