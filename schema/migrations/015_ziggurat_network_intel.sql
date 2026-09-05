-- Migration 015: Ziggurat Network Intelligence
--
-- Enables the Ziggurat crawler integration for CipherScan's network-wide
-- node census. Changes:
--   1. Relax nodes.observed_via CHECK to accept 'crawl' (hard blocker)
--   2. Add Tor columns to nodes + node_snapshots
--   3. Create node_edges table (topology)
--   4. Create node_metrics table (centrality)
--   5. Create nodes_crawl shadow table (parallel validation)
--
-- See decision record: wiki/decisions/007-ziggurat-crawler-integration.md

BEGIN;

-- 1. Relax observed_via constraint to allow 'crawl' source
ALTER TABLE public.nodes DROP CONSTRAINT nodes_observed_via_valid;
ALTER TABLE public.nodes ADD CONSTRAINT nodes_observed_via_valid
  CHECK ((observed_via)::text = ANY (ARRAY['peer','dns','crawl']::text[]));

-- 2. Tor columns on nodes (Category 2: hidden service support)
ALTER TABLE public.nodes ADD COLUMN IF NOT EXISTS onion_address varchar(62);
ALTER TABLE public.nodes ADD COLUMN IF NOT EXISTS tor_type varchar(10);

-- tor_type values: 'exit' (clearnet IP on Tor exit list),
--                  'hidden' (.onion, Phase 3-4 after handshake),
--                  'relay' (.onion seen via getaddr but not yet handshaked)

ALTER TABLE public.node_snapshots ADD COLUMN IF NOT EXISTS tor_hidden_nodes integer DEFAULT 0 NOT NULL;

-- 3. Network topology: edges between discovered nodes
CREATE TABLE IF NOT EXISTS public.node_edges (
    id bigserial PRIMARY KEY,
    src_addr_id bigint NOT NULL REFERENCES public.nodes(id) ON DELETE CASCADE,
    dst_addr_id bigint NOT NULL REFERENCES public.nodes(id) ON DELETE CASCADE,
    observed_at timestamptz DEFAULT now() NOT NULL,
    UNIQUE (src_addr_id, dst_addr_id)
);

ALTER TABLE public.node_edges OWNER TO postgres;

CREATE INDEX IF NOT EXISTS idx_node_edges_src ON public.node_edges(src_addr_id);
CREATE INDEX IF NOT EXISTS idx_node_edges_dst ON public.node_edges(dst_addr_id);

-- 4. Per-node centrality metrics
CREATE TABLE IF NOT EXISTS public.node_metrics (
    id bigserial PRIMARY KEY,
    addr_id bigint NOT NULL REFERENCES public.nodes(id) ON DELETE CASCADE,
    betweenness double precision,
    closeness double precision,
    degree integer,
    network_type varchar(16),
    observed_at timestamptz DEFAULT now() NOT NULL
);

ALTER TABLE public.node_metrics OWNER TO postgres;

CREATE INDEX IF NOT EXISTS idx_node_metrics_addr ON public.node_metrics(addr_id);
CREATE INDEX IF NOT EXISTS idx_node_metrics_observed ON public.node_metrics(observed_at DESC);

-- 5. Shadow table for parallel validation (same shape as nodes + crawl-specific)
CREATE TABLE IF NOT EXISTS public.nodes_crawl (
    id bigserial PRIMARY KEY,
    ip character varying(255) NOT NULL,
    port integer,
    country text,
    country_code character varying(2),
    city text,
    lat double precision,
    lon double precision,
    isp text,
    inbound boolean,
    ping_ms double precision,
    is_tor boolean DEFAULT false NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    user_agent character varying(255),
    client_impl character varying(64) DEFAULT 'Unknown'::character varying NOT NULL,
    client_version character varying(64),
    protocol_version integer,
    observed_via character varying(16) DEFAULT 'crawl'::character varying NOT NULL,
    first_seen timestamp with time zone DEFAULT now() NOT NULL,
    last_seen timestamp with time zone DEFAULT now() NOT NULL,
    onion_address varchar(62),
    tor_type varchar(10),
    betweenness double precision,
    closeness double precision,
    degree integer,
    network_type varchar(16),
    CONSTRAINT nodes_crawl_lat_valid CHECK (((lat IS NULL) OR ((lat >= (-90)::double precision) AND (lat <= (90)::double precision)))),
    CONSTRAINT nodes_crawl_lon_valid CHECK (((lon IS NULL) OR ((lon >= (-180)::double precision) AND (lon <= (180)::double precision)))),
    CONSTRAINT nodes_crawl_port_valid CHECK (((port IS NULL) OR ((port >= 1) AND (port <= 65535)))),
    CONSTRAINT nodes_crawl_ip_key UNIQUE (ip)
);

ALTER TABLE public.nodes_crawl OWNER TO postgres;

-- Grants for zcash_user (the app DB user)
GRANT ALL ON TABLE public.node_edges TO zcash_user;
GRANT ALL ON TABLE public.node_metrics TO zcash_user;
GRANT ALL ON TABLE public.nodes_crawl TO zcash_user;
GRANT ALL ON SEQUENCE public.node_edges_id_seq TO zcash_user;
GRANT ALL ON SEQUENCE public.node_metrics_id_seq TO zcash_user;
GRANT ALL ON SEQUENCE public.nodes_crawl_id_seq TO zcash_user;

-- Track this migration
INSERT INTO public.schema_migrations (version, description)
VALUES ('015', 'Ziggurat network intelligence: crawl source, Tor columns, node_edges, node_metrics, nodes_crawl shadow')
ON CONFLICT (version) DO NOTHING;

COMMIT;
