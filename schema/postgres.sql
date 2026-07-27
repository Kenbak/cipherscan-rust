--
-- PostgreSQL database dump
--


-- Dumped from database version 16.14 
-- Dumped by pg_dump version 16.14 

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: cleanup_expired_patterns(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.cleanup_expired_patterns() RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
  deleted_count INTEGER;
BEGIN
  DELETE FROM detected_patterns WHERE expires_at < NOW();
  GET DIAGNOSTICS deleted_count = ROW_COUNT;
  RETURN deleted_count;
END;
$$;


ALTER FUNCTION public.cleanup_expired_patterns() OWNER TO postgres;

--
-- Name: cleanup_expired_privacy_linkage(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.cleanup_expired_privacy_linkage() RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
  deleted_edges INTEGER;
  deleted_clusters INTEGER;
BEGIN
  DELETE FROM privacy_linkage_edges WHERE expires_at < NOW();
  GET DIAGNOSTICS deleted_edges = ROW_COUNT;
  DELETE FROM privacy_batch_clusters WHERE expires_at < NOW();
  GET DIAGNOSTICS deleted_clusters = ROW_COUNT;
  RETURN deleted_edges + deleted_clusters;
END;
$$;


ALTER FUNCTION public.cleanup_expired_privacy_linkage() OWNER TO postgres;

--
-- Name: update_address_timestamp(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.update_address_timestamp() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_address_timestamp() OWNER TO postgres;

--
-- Name: update_patterns_timestamp(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.update_patterns_timestamp() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_patterns_timestamp() OWNER TO postgres;

--
-- Name: update_privacy_linkage_timestamp(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.update_privacy_linkage_timestamp() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_privacy_linkage_timestamp() OWNER TO postgres;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: address_labels; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.address_labels (
    address character varying(96) NOT NULL,
    label character varying(100) NOT NULL,
    category character varying(50),
    description text,
    verified boolean DEFAULT false,
    logo_url character varying(255),
    source character varying(255),
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);


ALTER TABLE public.address_labels OWNER TO zcash_user;

--
-- Name: address_transactions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_transactions (
    address text NOT NULL,
    txid text NOT NULL,
    block_height integer NOT NULL,
    tx_index integer DEFAULT 0 NOT NULL,
    block_time bigint,
    is_input boolean DEFAULT false,
    is_output boolean DEFAULT false,
    value_in bigint DEFAULT 0,
    value_out bigint DEFAULT 0
);
ALTER TABLE ONLY public.address_transactions ALTER COLUMN txid SET STATISTICS 1000;


ALTER TABLE public.address_transactions OWNER TO postgres;

--
-- Name: addresses; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.addresses (
    address text NOT NULL,
    balance bigint DEFAULT 0,
    total_received bigint DEFAULT 0,
    total_sent bigint DEFAULT 0,
    tx_count integer DEFAULT 0,
    first_seen bigint,
    last_seen bigint,
    address_type text,
    updated_at timestamp without time zone DEFAULT now(),
    CONSTRAINT addresses_address_type_check CHECK ((address_type = ANY (ARRAY['transparent'::text, 'shielded'::text, 'unified'::text])))
);


ALTER TABLE public.addresses OWNER TO postgres;

--
-- Name: blocks; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.blocks (
    height bigint NOT NULL,
    hash text NOT NULL,
    "timestamp" bigint NOT NULL,
    version integer,
    merkle_root text,
    final_sapling_root text,
    bits text,
    nonce text,
    solution text,
    difficulty numeric,
    size integer,
    transaction_count integer DEFAULT 0,
    previous_block_hash text,
    total_fees bigint DEFAULT 0,
    miner_address text,
    created_at timestamp without time zone DEFAULT now(),
    confirmations integer,
    final_orchard_root text,
    final_ironwood_root text,
    coinbase_hex text
);


ALTER TABLE public.blocks OWNER TO postgres;

--
-- Name: boundary_pool_snapshots; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.boundary_pool_snapshots (
    boundary_height integer NOT NULL,
    block_time bigint NOT NULL,
    orchard_zat bigint NOT NULL,
    ironwood_zat bigint NOT NULL,
    sapling_zat bigint NOT NULL,
    sprout_zat bigint NOT NULL,
    transparent_zat bigint,
    chain_supply_zat bigint,
    created_at timestamp without time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.boundary_pool_snapshots OWNER TO postgres;

--
-- Name: chain_snapshots; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.chain_snapshots (
    id bigint NOT NULL,
    snapshot_time timestamp with time zone DEFAULT now() NOT NULL,
    block_height bigint NOT NULL,
    chain_size_bytes bigint DEFAULT 0 NOT NULL,
    chain_supply_zat bigint DEFAULT 0 NOT NULL,
    sprout_zat bigint DEFAULT 0 NOT NULL,
    sapling_zat bigint DEFAULT 0 NOT NULL,
    orchard_zat bigint DEFAULT 0 NOT NULL,
    ironwood_zat bigint DEFAULT 0 NOT NULL,
    transparent_zat bigint DEFAULT 0 NOT NULL
);


ALTER TABLE public.chain_snapshots OWNER TO zcash_user;

--
-- Name: chain_snapshots_id_seq; Type: SEQUENCE; Schema: public; Owner: zcash_user
--

CREATE SEQUENCE public.chain_snapshots_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.chain_snapshots_id_seq OWNER TO zcash_user;

--
-- Name: chain_snapshots_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: zcash_user
--

ALTER SEQUENCE public.chain_snapshots_id_seq OWNED BY public.chain_snapshots.id;


--
-- Name: cross_chain_swaps; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.cross_chain_swaps (
    id integer NOT NULL,
    deposit_address text NOT NULL,
    direction text NOT NULL,
    status text NOT NULL,
    source_chain text,
    source_token text,
    source_amount numeric,
    source_amount_usd numeric,
    source_tx_hashes text[],
    dest_chain text,
    dest_token text,
    dest_amount numeric,
    dest_amount_usd numeric,
    dest_tx_hashes text[],
    zec_txid text,
    zec_address text,
    near_tx_hashes text[],
    senders text[],
    recipient text,
    matched boolean DEFAULT false,
    match_attempts integer DEFAULT 0,
    swap_created_at timestamp with time zone,
    indexed_at timestamp with time zone DEFAULT now(),
    raw_origin_asset text,
    raw_dest_asset text
);


ALTER TABLE public.cross_chain_swaps OWNER TO postgres;

--
-- Name: cross_chain_swaps_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.cross_chain_swaps_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.cross_chain_swaps_id_seq OWNER TO postgres;

--
-- Name: cross_chain_swaps_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.cross_chain_swaps_id_seq OWNED BY public.cross_chain_swaps.id;


--
-- Name: detected_patterns; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.detected_patterns (
    id integer NOT NULL,
    pattern_type character varying(50) NOT NULL,
    pattern_hash character varying(64),
    score integer NOT NULL,
    warning_level character varying(10) NOT NULL,
    shield_txids text[],
    deshield_txids text[],
    total_amount_zat bigint,
    per_tx_amount_zat bigint,
    batch_count integer,
    first_tx_time integer,
    last_tx_time integer,
    time_span_hours numeric(10,2),
    metadata jsonb,
    detected_at timestamp with time zone DEFAULT now(),
    updated_at timestamp with time zone DEFAULT now(),
    expires_at timestamp with time zone DEFAULT (now() + '90 days'::interval),
    CONSTRAINT detected_patterns_score_check CHECK (((score >= 0) AND (score <= 100))),
    CONSTRAINT detected_patterns_warning_level_check CHECK (((warning_level)::text = ANY (ARRAY[('HIGH'::character varying)::text, ('MEDIUM'::character varying)::text, ('LOW'::character varying)::text])))
);


ALTER TABLE public.detected_patterns OWNER TO postgres;

--
-- Name: TABLE detected_patterns; Type: COMMENT; Schema: public; Owner: postgres
--

COMMENT ON TABLE public.detected_patterns IS 'Pre-computed privacy risk patterns detected by background scanner';


--
-- Name: COLUMN detected_patterns.pattern_hash; Type: COMMENT; Schema: public; Owner: postgres
--

COMMENT ON COLUMN public.detected_patterns.pattern_hash IS 'SHA256 of sorted txids to prevent duplicate detection';


--
-- Name: COLUMN detected_patterns.metadata; Type: COMMENT; Schema: public; Owner: postgres
--

COMMENT ON COLUMN public.detected_patterns.metadata IS 'Full pattern details including breakdown and explanation';


--
-- Name: detected_patterns_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.detected_patterns_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.detected_patterns_id_seq OWNER TO postgres;

--
-- Name: detected_patterns_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.detected_patterns_id_seq OWNED BY public.detected_patterns.id;


--
-- Name: fork_events; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.fork_events (
    id integer NOT NULL,
    fork_height bigint NOT NULL,
    depth integer DEFAULT 1 NOT NULL,
    canonical_tip bigint,
    orphaned_count integer DEFAULT 0 NOT NULL,
    source text DEFAULT 'internal'::text NOT NULL,
    description text,
    detected_at timestamp without time zone DEFAULT now(),
    resolved_at timestamp without time zone
);


ALTER TABLE public.fork_events OWNER TO postgres;

--
-- Name: fork_events_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.fork_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.fork_events_id_seq OWNER TO postgres;

--
-- Name: fork_events_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.fork_events_id_seq OWNED BY public.fork_events.id;


--
-- Name: fork_monitor_nodes; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.fork_monitor_nodes (
    name text NOT NULL,
    tip integer NOT NULL,
    tip_hash text,
    sample_hashes jsonb DEFAULT '[]'::jsonb,
    peers integer,
    mining boolean,
    ttl text DEFAULT '24h'::text NOT NULL,
    reported_at bigint NOT NULL
);


ALTER TABLE public.fork_monitor_nodes OWNER TO postgres;

--
-- Name: indexer_state; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.indexer_state (
    key text NOT NULL,
    value text NOT NULL,
    updated_at timestamp with time zone DEFAULT now()
);


ALTER TABLE public.indexer_state OWNER TO zcash_user;

--
-- Name: miner_destination_daily; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.miner_destination_daily (
    date date NOT NULL,
    pool_name text NOT NULL,
    shielded_zat bigint DEFAULT 0 NOT NULL,
    exchange_zat bigint DEFAULT 0 NOT NULL,
    bridge_zat bigint DEFAULT 0 NOT NULL,
    other_zat bigint DEFAULT 0 NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.miner_destination_daily OWNER TO zcash_user;

--
-- Name: mining_behavior_daily; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.mining_behavior_daily (
    date date NOT NULL,
    pool_name text NOT NULL,
    miner_address text NOT NULL,
    earned_zat bigint DEFAULT 0 NOT NULL,
    spent_zat bigint DEFAULT 0 NOT NULL,
    held_zat bigint DEFAULT 0 NOT NULL,
    blocks_mined integer DEFAULT 0 NOT NULL,
    outputs_spent integer DEFAULT 0 NOT NULL,
    outputs_total integer DEFAULT 0 NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.mining_behavior_daily OWNER TO zcash_user;

--
-- Name: transactions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.transactions (
    txid text NOT NULL,
    block_height bigint NOT NULL,
    block_hash text NOT NULL,
    version integer,
    locktime bigint,
    size integer,
    fee bigint DEFAULT 0,
    total_input bigint DEFAULT 0,
    total_output bigint DEFAULT 0,
    orchard_actions integer DEFAULT 0,
    value_balance bigint DEFAULT 0,
    value_balance_sapling bigint DEFAULT 0,
    value_balance_orchard bigint DEFAULT 0,
    has_shielded_data boolean DEFAULT false,
    is_coinbase boolean DEFAULT false,
    created_at timestamp without time zone DEFAULT now(),
    block_time bigint NOT NULL,
    vin_count integer DEFAULT 0,
    vout_count integer DEFAULT 0,
    tx_index integer,
    has_sapling boolean DEFAULT false,
    has_orchard boolean DEFAULT false,
    has_sprout boolean DEFAULT false,
    expiry_height integer,
    sapling_spend_count integer DEFAULT 0,
    sapling_output_count integer DEFAULT 0,
    sprout_joinsplit_count integer DEFAULT 0,
    privacy_score smallint,
    flow_type text,
    has_ironwood boolean DEFAULT false,
    ironwood_actions integer DEFAULT 0,
    value_balance_ironwood bigint DEFAULT 0
);


ALTER TABLE public.transactions OWNER TO postgres;

--
-- Name: mv_crosschain_latency; Type: MATERIALIZED VIEW; Schema: public; Owner: zcash_user
--

CREATE MATERIALIZED VIEW public.mv_crosschain_latency AS
 SELECT direction,
    chain,
    (count(*))::integer AS swap_count,
    (avg(latency_min))::double precision AS avg_minutes,
    percentile_cont((0.5)::double precision) WITHIN GROUP (ORDER BY ((latency_min)::double precision)) AS median_minutes
   FROM ( SELECT ccs.direction,
                CASE
                    WHEN (ccs.direction = 'inflow'::text) THEN ccs.source_chain
                    ELSE ccs.dest_chain
                END AS chain,
            (((t.block_time)::numeric - EXTRACT(epoch FROM ccs.swap_created_at)) / 60.0) AS latency_min
           FROM (public.cross_chain_swaps ccs
             JOIN public.transactions t ON ((t.txid = ccs.zec_txid)))
          WHERE ((ccs.status = 'SUCCESS'::text) AND (ccs.matched = true) AND (ccs.zec_txid IS NOT NULL) AND (((t.block_time)::numeric - EXTRACT(epoch FROM ccs.swap_created_at)) > (0)::numeric) AND (((t.block_time)::numeric - EXTRACT(epoch FROM ccs.swap_created_at)) < (86400)::numeric))) sub
  GROUP BY direction, chain
  WITH NO DATA;


ALTER MATERIALIZED VIEW public.mv_crosschain_latency OWNER TO zcash_user;

--
-- Name: mv_crosschain_popular_pairs; Type: MATERIALIZED VIEW; Schema: public; Owner: zcash_user
--

CREATE MATERIALIZED VIEW public.mv_crosschain_popular_pairs AS
 SELECT
        CASE
            WHEN (direction = 'inflow'::text) THEN source_chain
            ELSE dest_chain
        END AS chain,
        CASE
            WHEN (direction = 'inflow'::text) THEN source_token
            ELSE dest_token
        END AS token,
    (count(*))::integer AS swap_count
   FROM public.cross_chain_swaps
  WHERE ((status = 'SUCCESS'::text) AND (swap_created_at >= (now() - '30 days'::interval)) AND (source_token <> ALL (ARRAY['UNKNOWN_TOKEN'::text, 'UNKNOWN'::text, 'OTHER'::text])) AND (dest_token <> ALL (ARRAY['UNKNOWN_TOKEN'::text, 'UNKNOWN'::text, 'OTHER'::text])))
  GROUP BY
        CASE
            WHEN (direction = 'inflow'::text) THEN source_chain
            ELSE dest_chain
        END,
        CASE
            WHEN (direction = 'inflow'::text) THEN source_token
            ELSE dest_token
        END
  ORDER BY ((count(*))::integer) DESC
 LIMIT 100
  WITH NO DATA;


ALTER MATERIALIZED VIEW public.mv_crosschain_popular_pairs OWNER TO zcash_user;

--
-- Name: mv_crosschain_summary; Type: MATERIALIZED VIEW; Schema: public; Owner: zcash_user
--

CREATE MATERIALIZED VIEW public.mv_crosschain_summary AS
 SELECT (count(*) FILTER (WHERE (swap_created_at >= (now() - '24:00:00'::interval))))::integer AS swaps_24h,
    (COALESCE(sum(source_amount_usd) FILTER (WHERE (swap_created_at >= (now() - '24:00:00'::interval))), (0)::numeric))::double precision AS volume_24h,
    (count(*))::integer AS swaps_all_time,
    (COALESCE(sum(source_amount_usd), (0)::numeric))::double precision AS volume_all_time
   FROM public.cross_chain_swaps
  WHERE (status = 'SUCCESS'::text)
  WITH NO DATA;


ALTER MATERIALIZED VIEW public.mv_crosschain_summary OWNER TO zcash_user;

--
-- Name: mv_crosschain_trends; Type: MATERIALIZED VIEW; Schema: public; Owner: zcash_user
--

CREATE MATERIALIZED VIEW public.mv_crosschain_trends AS
 SELECT (date_trunc('day'::text, swap_created_at))::date AS day,
    direction,
    (count(*))::integer AS swap_count,
    (COALESCE(sum(source_amount_usd), (0)::numeric))::double precision AS volume_usd
   FROM public.cross_chain_swaps
  WHERE (status = 'SUCCESS'::text)
  GROUP BY ((date_trunc('day'::text, swap_created_at))::date), direction
  ORDER BY ((date_trunc('day'::text, swap_created_at))::date)
  WITH NO DATA;


ALTER MATERIALIZED VIEW public.mv_crosschain_trends OWNER TO zcash_user;

--
-- Name: mv_crosschain_volume_24h; Type: MATERIALIZED VIEW; Schema: public; Owner: zcash_user
--

CREATE MATERIALIZED VIEW public.mv_crosschain_volume_24h AS
 SELECT direction,
        CASE
            WHEN (direction = 'inflow'::text) THEN source_chain
            ELSE dest_chain
        END AS chain,
        CASE
            WHEN (direction = 'inflow'::text) THEN source_token
            ELSE dest_token
        END AS token,
    (COALESCE(sum(source_amount_usd), (0)::numeric))::double precision AS volume_usd,
    (count(*))::integer AS swap_count
   FROM public.cross_chain_swaps
  WHERE ((status = 'SUCCESS'::text) AND (swap_created_at >= (now() - '24:00:00'::interval)))
  GROUP BY direction,
        CASE
            WHEN (direction = 'inflow'::text) THEN source_chain
            ELSE dest_chain
        END,
        CASE
            WHEN (direction = 'inflow'::text) THEN source_token
            ELSE dest_token
        END
  WITH NO DATA;


ALTER MATERIALIZED VIEW public.mv_crosschain_volume_24h OWNER TO zcash_user;

--
-- Name: node_snapshots; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.node_snapshots (
    id bigint NOT NULL,
    snapshot_time timestamp with time zone DEFAULT now() NOT NULL,
    active_nodes integer DEFAULT 0 NOT NULL,
    total_nodes integer DEFAULT 0 NOT NULL,
    countries integer DEFAULT 0 NOT NULL,
    tor_nodes integer DEFAULT 0 NOT NULL,
    inbound_nodes integer DEFAULT 0 NOT NULL,
    outbound_nodes integer DEFAULT 0 NOT NULL,
    avg_ping_ms double precision,
    identified_client_nodes integer DEFAULT 0 NOT NULL,
    client_counts jsonb DEFAULT '{}'::jsonb NOT NULL
);


ALTER TABLE public.node_snapshots OWNER TO postgres;

--
-- Name: node_snapshots_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.node_snapshots_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.node_snapshots_id_seq OWNER TO postgres;

--
-- Name: node_snapshots_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.node_snapshots_id_seq OWNED BY public.node_snapshots.id;


--
-- Name: nodes; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.nodes (
    id bigint NOT NULL,
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
    observed_via character varying(16) DEFAULT 'peer'::character varying NOT NULL,
    first_seen timestamp with time zone DEFAULT now() NOT NULL,
    last_seen timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT nodes_lat_valid CHECK (((lat IS NULL) OR ((lat >= ('-90'::integer)::double precision) AND (lat <= (90)::double precision)))),
    CONSTRAINT nodes_lon_valid CHECK (((lon IS NULL) OR ((lon >= ('-180'::integer)::double precision) AND (lon <= (180)::double precision)))),
    CONSTRAINT nodes_observed_via_valid CHECK (((observed_via)::text = ANY ((ARRAY['peer'::character varying, 'dns'::character varying])::text[]))),
    CONSTRAINT nodes_port_valid CHECK (((port IS NULL) OR ((port >= 1) AND (port <= 65535))))
);


ALTER TABLE public.nodes OWNER TO postgres;

--
-- Name: nodes_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.nodes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.nodes_id_seq OWNER TO postgres;

--
-- Name: nodes_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.nodes_id_seq OWNED BY public.nodes.id;


--
-- Name: orphaned_blocks; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.orphaned_blocks (
    id integer NOT NULL,
    height bigint NOT NULL,
    hash text NOT NULL,
    canonical_hash text,
    "timestamp" bigint,
    transaction_count integer DEFAULT 0,
    size integer DEFAULT 0,
    difficulty text,
    miner_address text,
    previous_block_hash text,
    fork_event_id integer,
    source text DEFAULT 'internal'::text NOT NULL,
    reported_by text,
    consensus_valid boolean,
    detected_at timestamp without time zone DEFAULT now(),
    final_sapling_root text,
    final_orchard_root text,
    coinbase_hex text,
    block_header_hex text,
    first_indexed_at timestamp without time zone
);


ALTER TABLE public.orphaned_blocks OWNER TO postgres;

--
-- Name: orphaned_blocks_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.orphaned_blocks_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.orphaned_blocks_id_seq OWNER TO postgres;

--
-- Name: orphaned_blocks_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.orphaned_blocks_id_seq OWNED BY public.orphaned_blocks.id;


--
-- Name: orphaned_transaction_inputs; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.orphaned_transaction_inputs (
    id bigint NOT NULL,
    txid text NOT NULL,
    block_hash text NOT NULL,
    vout_index integer,
    prev_txid text,
    prev_vout integer,
    address text,
    value bigint,
    coinbase text
);


ALTER TABLE public.orphaned_transaction_inputs OWNER TO postgres;

--
-- Name: orphaned_transaction_inputs_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.orphaned_transaction_inputs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.orphaned_transaction_inputs_id_seq OWNER TO postgres;

--
-- Name: orphaned_transaction_inputs_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.orphaned_transaction_inputs_id_seq OWNED BY public.orphaned_transaction_inputs.id;


--
-- Name: orphaned_transaction_outputs; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.orphaned_transaction_outputs (
    id bigint NOT NULL,
    txid text NOT NULL,
    block_hash text NOT NULL,
    vout_index integer,
    value bigint,
    address text,
    script_type text
);


ALTER TABLE public.orphaned_transaction_outputs OWNER TO postgres;

--
-- Name: orphaned_transaction_outputs_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.orphaned_transaction_outputs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.orphaned_transaction_outputs_id_seq OWNER TO postgres;

--
-- Name: orphaned_transaction_outputs_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.orphaned_transaction_outputs_id_seq OWNED BY public.orphaned_transaction_outputs.id;


--
-- Name: orphaned_transactions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.orphaned_transactions (
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
    fork_event_id integer,
    archived_at timestamp without time zone DEFAULT now(),
    first_indexed_at timestamp without time zone
);


ALTER TABLE public.orphaned_transactions OWNER TO postgres;

--
-- Name: privacy_batch_clusters; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.privacy_batch_clusters (
    id integer NOT NULL,
    cluster_hash character varying(64) NOT NULL,
    cluster_type character varying(32) NOT NULL,
    anchor_txid text,
    anchor_block_height integer,
    anchor_block_time integer,
    anchor_amount_zat bigint,
    member_txids text[] NOT NULL,
    member_count integer NOT NULL,
    total_amount_zat bigint NOT NULL,
    representative_amount_zat bigint NOT NULL,
    first_tx_time integer NOT NULL,
    last_tx_time integer NOT NULL,
    time_span_seconds integer NOT NULL,
    confidence_score integer NOT NULL,
    confidence_margin integer DEFAULT 0 NOT NULL,
    ambiguity_score integer DEFAULT 0 NOT NULL,
    warning_level character varying(10) NOT NULL,
    evidence jsonb DEFAULT '{}'::jsonb NOT NULL,
    detected_at timestamp with time zone DEFAULT now(),
    updated_at timestamp with time zone DEFAULT now(),
    expires_at timestamp with time zone DEFAULT (now() + '90 days'::interval),
    CONSTRAINT privacy_batch_clusters_ambiguity_score_check CHECK (((ambiguity_score >= 0) AND (ambiguity_score <= 100))),
    CONSTRAINT privacy_batch_clusters_cluster_type_check CHECK (((cluster_type)::text = ANY (ARRAY[('BATCH_DESHIELD'::character varying)::text]))),
    CONSTRAINT privacy_batch_clusters_confidence_margin_check CHECK ((confidence_margin >= 0)),
    CONSTRAINT privacy_batch_clusters_confidence_score_check CHECK (((confidence_score >= 0) AND (confidence_score <= 100))),
    CONSTRAINT privacy_batch_clusters_member_count_check CHECK ((member_count >= 2)),
    CONSTRAINT privacy_batch_clusters_warning_level_check CHECK (((warning_level)::text = ANY (ARRAY[('HIGH'::character varying)::text, ('MEDIUM'::character varying)::text, ('LOW'::character varying)::text])))
);


ALTER TABLE public.privacy_batch_clusters OWNER TO postgres;

--
-- Name: privacy_batch_clusters_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

ALTER TABLE public.privacy_batch_clusters ALTER COLUMN id ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.privacy_batch_clusters_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: privacy_linkage_edges; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.privacy_linkage_edges (
    id integer NOT NULL,
    edge_hash character varying(64) NOT NULL,
    edge_type character varying(32) NOT NULL,
    candidate_rank integer DEFAULT 1 NOT NULL,
    src_txid text NOT NULL,
    src_block_height integer,
    src_block_time integer NOT NULL,
    src_amount_zat bigint NOT NULL,
    src_pool text,
    dst_txid text NOT NULL,
    dst_block_height integer,
    dst_block_time integer NOT NULL,
    dst_amount_zat bigint NOT NULL,
    dst_pool text,
    anchor_txid text,
    amount_diff_zat bigint DEFAULT 0 NOT NULL,
    time_delta_seconds integer NOT NULL,
    amount_rarity_score numeric(6,2) DEFAULT 0 NOT NULL,
    amount_weirdness_score numeric(6,2) DEFAULT 0 NOT NULL,
    timing_score numeric(6,2) DEFAULT 0 NOT NULL,
    recipient_reuse_score numeric(6,2) DEFAULT 0 NOT NULL,
    confidence_score integer NOT NULL,
    confidence_margin integer DEFAULT 0 NOT NULL,
    ambiguity_score integer DEFAULT 0 NOT NULL,
    warning_level character varying(10) NOT NULL,
    evidence jsonb DEFAULT '{}'::jsonb NOT NULL,
    detected_at timestamp with time zone DEFAULT now(),
    updated_at timestamp with time zone DEFAULT now(),
    expires_at timestamp with time zone DEFAULT (now() + '90 days'::interval),
    CONSTRAINT privacy_linkage_edges_ambiguity_score_check CHECK (((ambiguity_score >= 0) AND (ambiguity_score <= 100))),
    CONSTRAINT privacy_linkage_edges_candidate_rank_check CHECK ((candidate_rank >= 1)),
    CONSTRAINT privacy_linkage_edges_confidence_margin_check CHECK ((confidence_margin >= 0)),
    CONSTRAINT privacy_linkage_edges_confidence_score_check CHECK (((confidence_score >= 0) AND (confidence_score <= 100))),
    CONSTRAINT privacy_linkage_edges_edge_type_check CHECK (((edge_type)::text = ANY (ARRAY[('PAIR_LINK'::character varying)::text, ('BATCH_LINK'::character varying)::text]))),
    CONSTRAINT privacy_linkage_edges_warning_level_check CHECK (((warning_level)::text = ANY (ARRAY[('HIGH'::character varying)::text, ('MEDIUM'::character varying)::text, ('LOW'::character varying)::text])))
);


ALTER TABLE public.privacy_linkage_edges OWNER TO postgres;

--
-- Name: privacy_linkage_edges_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

ALTER TABLE public.privacy_linkage_edges ALTER COLUMN id ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.privacy_linkage_edges_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: privacy_stats; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.privacy_stats (
    id integer NOT NULL,
    total_blocks bigint DEFAULT 0 NOT NULL,
    total_transactions bigint DEFAULT 0 NOT NULL,
    shielded_tx bigint DEFAULT 0 NOT NULL,
    transparent_tx bigint DEFAULT 0 NOT NULL,
    coinbase_tx bigint DEFAULT 0 NOT NULL,
    mixed_tx bigint DEFAULT 0 NOT NULL,
    fully_shielded_tx bigint DEFAULT 0 NOT NULL,
    shielded_pool_size bigint DEFAULT 0 NOT NULL,
    total_shielded bigint DEFAULT 0 NOT NULL,
    total_unshielded bigint DEFAULT 0 NOT NULL,
    shielded_percentage numeric(10,6) DEFAULT 0 NOT NULL,
    privacy_score integer DEFAULT 0 NOT NULL,
    avg_shielded_per_day numeric(10,2) DEFAULT 0 NOT NULL,
    adoption_trend character varying(20) DEFAULT 'stable'::character varying NOT NULL,
    last_block_scanned bigint DEFAULT 0 NOT NULL,
    calculation_duration_ms integer,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    sprout_pool_size bigint DEFAULT 0,
    sapling_pool_size bigint DEFAULT 0,
    orchard_pool_size bigint DEFAULT 0,
    transparent_pool_size bigint DEFAULT 0,
    chain_supply bigint DEFAULT 0,
    ironwood_pool_size bigint DEFAULT 0 NOT NULL,
    privacy_score_breakdown jsonb
);


ALTER TABLE public.privacy_stats OWNER TO postgres;

--
-- Name: privacy_stats_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.privacy_stats_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.privacy_stats_id_seq OWNER TO postgres;

--
-- Name: privacy_stats_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.privacy_stats_id_seq OWNED BY public.privacy_stats.id;


--
-- Name: privacy_trends_daily; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.privacy_trends_daily (
    id integer NOT NULL,
    date date NOT NULL,
    shielded_count bigint DEFAULT 0 NOT NULL,
    transparent_count bigint DEFAULT 0 NOT NULL,
    shielded_percentage numeric(10,6) DEFAULT 0 NOT NULL,
    pool_size bigint DEFAULT 0 NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    privacy_score integer DEFAULT 0,
    sprout_pool_size bigint DEFAULT 0 NOT NULL,
    sapling_pool_size bigint DEFAULT 0 NOT NULL,
    orchard_pool_size bigint DEFAULT 0 NOT NULL,
    ironwood_pool_size bigint DEFAULT 0 NOT NULL,
    transparent_pool_size bigint DEFAULT 0 NOT NULL,
    chain_supply bigint DEFAULT 0 NOT NULL
);


ALTER TABLE public.privacy_trends_daily OWNER TO postgres;

--
-- Name: privacy_trends_daily_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.privacy_trends_daily_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.privacy_trends_daily_id_seq OWNER TO postgres;

--
-- Name: privacy_trends_daily_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.privacy_trends_daily_id_seq OWNED BY public.privacy_trends_daily.id;


--
-- Name: shielded_flows; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.shielded_flows (
    id integer NOT NULL,
    txid text NOT NULL,
    block_height integer NOT NULL,
    block_time integer NOT NULL,
    flow_type text NOT NULL,
    amount_zat bigint NOT NULL,
    pool text NOT NULL,
    amount_sapling_zat bigint DEFAULT 0,
    amount_orchard_zat bigint DEFAULT 0,
    transparent_addresses text[],
    transparent_value_zat bigint DEFAULT 0,
    created_at timestamp without time zone DEFAULT now(),
    sapling_spend_count integer DEFAULT 0,
    sapling_output_count integer DEFAULT 0,
    orchard_action_count integer DEFAULT 0,
    is_pool_migration boolean DEFAULT false,
    migration_from_pool text,
    migration_to_pool text,
    CONSTRAINT shielded_flows_flow_type_check CHECK ((flow_type = ANY (ARRAY['shield'::text, 'deshield'::text]))),
    CONSTRAINT shielded_flows_pool_check CHECK ((pool = ANY (ARRAY['sapling'::text, 'orchard'::text, 'sprout'::text, 'mixed'::text, 'ironwood'::text])))
);


ALTER TABLE public.shielded_flows OWNER TO zcash_user;

--
-- Name: shielded_flows_id_seq; Type: SEQUENCE; Schema: public; Owner: zcash_user
--

CREATE SEQUENCE public.shielded_flows_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.shielded_flows_id_seq OWNER TO zcash_user;

--
-- Name: shielded_flows_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: zcash_user
--

ALTER SEQUENCE public.shielded_flows_id_seq OWNED BY public.shielded_flows.id;


--
-- Name: swap_amount_stats_daily; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.swap_amount_stats_daily (
    date date NOT NULL,
    source_chain text NOT NULL,
    source_token text NOT NULL,
    amount_bucket numeric NOT NULL,
    swap_count integer NOT NULL,
    total_volume_usd numeric
);


ALTER TABLE public.swap_amount_stats_daily OWNER TO postgres;

--
-- Name: sync_state; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.sync_state (
    job_name text NOT NULL,
    last_sync_timestamp timestamp with time zone,
    last_sync_count integer DEFAULT 0,
    updated_at timestamp with time zone DEFAULT now()
);


ALTER TABLE public.sync_state OWNER TO postgres;

--
-- Name: tip_reports; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.tip_reports (
    id integer NOT NULL,
    height bigint NOT NULL,
    hash text NOT NULL,
    node_id text,
    ip_hash text,
    is_match boolean,
    reported_at timestamp without time zone DEFAULT now()
);


ALTER TABLE public.tip_reports OWNER TO postgres;

--
-- Name: tip_reports_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.tip_reports_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.tip_reports_id_seq OWNER TO postgres;

--
-- Name: tip_reports_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.tip_reports_id_seq OWNED BY public.tip_reports.id;


--
-- Name: trading_signals; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.trading_signals (
    id bigint NOT NULL,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    signal_date date NOT NULL,
    svr_7d numeric,
    svr_30d numeric,
    pool_momentum numeric,
    miner_pressure numeric,
    crosschain_flow numeric,
    shielded_tx_momentum numeric,
    composite_score numeric NOT NULL,
    signal text NOT NULL,
    price_usd numeric,
    shielded_pool_pct numeric,
    notes text
);


ALTER TABLE public.trading_signals OWNER TO zcash_user;

--
-- Name: trading_signals_id_seq; Type: SEQUENCE; Schema: public; Owner: zcash_user
--

CREATE SEQUENCE public.trading_signals_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.trading_signals_id_seq OWNER TO zcash_user;

--
-- Name: trading_signals_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: zcash_user
--

ALTER SEQUENCE public.trading_signals_id_seq OWNED BY public.trading_signals.id;


--
-- Name: transaction_inputs; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.transaction_inputs (
    txid text NOT NULL,
    vout_index integer NOT NULL,
    prev_txid text,
    prev_vout integer,
    script_sig text,
    sequence bigint,
    address text,
    value bigint,
    created_at timestamp without time zone DEFAULT now(),
    coinbase text
);


ALTER TABLE public.transaction_inputs OWNER TO zcash_user;

--
-- Name: transaction_outputs; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.transaction_outputs (
    txid text NOT NULL,
    vout_index integer NOT NULL,
    value bigint,
    script_pubkey text,
    address text,
    spent boolean DEFAULT false,
    spent_txid text,
    spent_at timestamp without time zone,
    created_at timestamp without time zone DEFAULT now(),
    script_type text
);


ALTER TABLE public.transaction_outputs OWNER TO zcash_user;

--
-- Name: turnstile_daily; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.turnstile_daily (
    date date NOT NULL,
    pool text NOT NULL,
    deshielded_zat bigint DEFAULT 0 NOT NULL,
    held_zat bigint DEFAULT 0 NOT NULL,
    reshielded_zat bigint DEFAULT 0 NOT NULL,
    exchange_zat bigint DEFAULT 0 NOT NULL,
    bridge_zat bigint DEFAULT 0 NOT NULL,
    transferred_zat bigint DEFAULT 0 NOT NULL,
    tx_count integer DEFAULT 0 NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


ALTER TABLE public.turnstile_daily OWNER TO zcash_user;

--
-- Name: zec_price_daily; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.zec_price_daily (
    date date NOT NULL,
    price_usd numeric(12,4) NOT NULL,
    market_cap_usd bigint,
    volume_usd bigint,
    source character varying(50) DEFAULT 'coingecko'::character varying,
    created_at timestamp without time zone DEFAULT now()
);


ALTER TABLE public.zec_price_daily OWNER TO zcash_user;

--
-- Name: chain_snapshots id; Type: DEFAULT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.chain_snapshots ALTER COLUMN id SET DEFAULT nextval('public.chain_snapshots_id_seq'::regclass);


--
-- Name: cross_chain_swaps id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.cross_chain_swaps ALTER COLUMN id SET DEFAULT nextval('public.cross_chain_swaps_id_seq'::regclass);


--
-- Name: detected_patterns id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.detected_patterns ALTER COLUMN id SET DEFAULT nextval('public.detected_patterns_id_seq'::regclass);


--
-- Name: fork_events id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.fork_events ALTER COLUMN id SET DEFAULT nextval('public.fork_events_id_seq'::regclass);


--
-- Name: node_snapshots id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.node_snapshots ALTER COLUMN id SET DEFAULT nextval('public.node_snapshots_id_seq'::regclass);


--
-- Name: nodes id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.nodes ALTER COLUMN id SET DEFAULT nextval('public.nodes_id_seq'::regclass);


--
-- Name: orphaned_blocks id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_blocks ALTER COLUMN id SET DEFAULT nextval('public.orphaned_blocks_id_seq'::regclass);


--
-- Name: orphaned_transaction_inputs id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_transaction_inputs ALTER COLUMN id SET DEFAULT nextval('public.orphaned_transaction_inputs_id_seq'::regclass);


--
-- Name: orphaned_transaction_outputs id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_transaction_outputs ALTER COLUMN id SET DEFAULT nextval('public.orphaned_transaction_outputs_id_seq'::regclass);


--
-- Name: privacy_stats id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_stats ALTER COLUMN id SET DEFAULT nextval('public.privacy_stats_id_seq'::regclass);


--
-- Name: privacy_trends_daily id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_trends_daily ALTER COLUMN id SET DEFAULT nextval('public.privacy_trends_daily_id_seq'::regclass);


--
-- Name: shielded_flows id; Type: DEFAULT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.shielded_flows ALTER COLUMN id SET DEFAULT nextval('public.shielded_flows_id_seq'::regclass);


--
-- Name: tip_reports id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.tip_reports ALTER COLUMN id SET DEFAULT nextval('public.tip_reports_id_seq'::regclass);


--
-- Name: trading_signals id; Type: DEFAULT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.trading_signals ALTER COLUMN id SET DEFAULT nextval('public.trading_signals_id_seq'::regclass);


--
-- Name: address_labels address_labels_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.address_labels
    ADD CONSTRAINT address_labels_pkey PRIMARY KEY (address);


--
-- Name: address_transactions address_transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_transactions
    ADD CONSTRAINT address_transactions_pkey PRIMARY KEY (address, block_height, tx_index, txid);


--
-- Name: addresses addresses_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.addresses
    ADD CONSTRAINT addresses_pkey PRIMARY KEY (address);


--
-- Name: blocks blocks_hash_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.blocks
    ADD CONSTRAINT blocks_hash_key UNIQUE (hash);


--
-- Name: blocks blocks_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.blocks
    ADD CONSTRAINT blocks_pkey PRIMARY KEY (height);


--
-- Name: boundary_pool_snapshots boundary_pool_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.boundary_pool_snapshots
    ADD CONSTRAINT boundary_pool_snapshots_pkey PRIMARY KEY (boundary_height);


--
-- Name: chain_snapshots chain_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.chain_snapshots
    ADD CONSTRAINT chain_snapshots_pkey PRIMARY KEY (id);


--
-- Name: cross_chain_swaps cross_chain_swaps_deposit_address_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.cross_chain_swaps
    ADD CONSTRAINT cross_chain_swaps_deposit_address_key UNIQUE (deposit_address);


--
-- Name: cross_chain_swaps cross_chain_swaps_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.cross_chain_swaps
    ADD CONSTRAINT cross_chain_swaps_pkey PRIMARY KEY (id);


--
-- Name: detected_patterns detected_patterns_pattern_hash_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.detected_patterns
    ADD CONSTRAINT detected_patterns_pattern_hash_key UNIQUE (pattern_hash);


--
-- Name: detected_patterns detected_patterns_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.detected_patterns
    ADD CONSTRAINT detected_patterns_pkey PRIMARY KEY (id);


--
-- Name: fork_events fork_events_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.fork_events
    ADD CONSTRAINT fork_events_pkey PRIMARY KEY (id);


--
-- Name: fork_monitor_nodes fork_monitor_nodes_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.fork_monitor_nodes
    ADD CONSTRAINT fork_monitor_nodes_pkey PRIMARY KEY (name);


--
-- Name: indexer_state indexer_state_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.indexer_state
    ADD CONSTRAINT indexer_state_pkey PRIMARY KEY (key);


--
-- Name: miner_destination_daily miner_destination_daily_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.miner_destination_daily
    ADD CONSTRAINT miner_destination_daily_pkey PRIMARY KEY (date, pool_name);


--
-- Name: mining_behavior_daily mining_behavior_daily_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.mining_behavior_daily
    ADD CONSTRAINT mining_behavior_daily_pkey PRIMARY KEY (date, pool_name);


--
-- Name: node_snapshots node_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.node_snapshots
    ADD CONSTRAINT node_snapshots_pkey PRIMARY KEY (id);


--
-- Name: nodes nodes_ip_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.nodes
    ADD CONSTRAINT nodes_ip_key UNIQUE (ip);


--
-- Name: nodes nodes_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.nodes
    ADD CONSTRAINT nodes_pkey PRIMARY KEY (id);


--
-- Name: orphaned_blocks orphaned_blocks_hash_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_blocks
    ADD CONSTRAINT orphaned_blocks_hash_key UNIQUE (hash);


--
-- Name: orphaned_blocks orphaned_blocks_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_blocks
    ADD CONSTRAINT orphaned_blocks_pkey PRIMARY KEY (id);


--
-- Name: orphaned_transaction_inputs orphaned_transaction_inputs_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_transaction_inputs
    ADD CONSTRAINT orphaned_transaction_inputs_pkey PRIMARY KEY (id);


--
-- Name: orphaned_transaction_outputs orphaned_transaction_outputs_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_transaction_outputs
    ADD CONSTRAINT orphaned_transaction_outputs_pkey PRIMARY KEY (id);


--
-- Name: orphaned_transactions orphaned_transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_transactions
    ADD CONSTRAINT orphaned_transactions_pkey PRIMARY KEY (txid, block_hash);


--
-- Name: privacy_batch_clusters privacy_batch_clusters_cluster_hash_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_batch_clusters
    ADD CONSTRAINT privacy_batch_clusters_cluster_hash_key UNIQUE (cluster_hash);


--
-- Name: privacy_batch_clusters privacy_batch_clusters_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_batch_clusters
    ADD CONSTRAINT privacy_batch_clusters_pkey PRIMARY KEY (id);


--
-- Name: privacy_linkage_edges privacy_linkage_edges_edge_hash_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_linkage_edges
    ADD CONSTRAINT privacy_linkage_edges_edge_hash_key UNIQUE (edge_hash);


--
-- Name: privacy_linkage_edges privacy_linkage_edges_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_linkage_edges
    ADD CONSTRAINT privacy_linkage_edges_pkey PRIMARY KEY (id);


--
-- Name: privacy_stats privacy_stats_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_stats
    ADD CONSTRAINT privacy_stats_pkey PRIMARY KEY (id);


--
-- Name: privacy_trends_daily privacy_trends_daily_date_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_trends_daily
    ADD CONSTRAINT privacy_trends_daily_date_key UNIQUE (date);


--
-- Name: privacy_trends_daily privacy_trends_daily_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.privacy_trends_daily
    ADD CONSTRAINT privacy_trends_daily_pkey PRIMARY KEY (id);


--
-- Name: shielded_flows shielded_flows_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.shielded_flows
    ADD CONSTRAINT shielded_flows_pkey PRIMARY KEY (id);


--
-- Name: shielded_flows shielded_flows_txid_flow_unique; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.shielded_flows
    ADD CONSTRAINT shielded_flows_txid_flow_unique UNIQUE (txid, flow_type);


--
-- Name: swap_amount_stats_daily swap_amount_stats_daily_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.swap_amount_stats_daily
    ADD CONSTRAINT swap_amount_stats_daily_pkey PRIMARY KEY (date, source_chain, source_token, amount_bucket);


--
-- Name: sync_state sync_state_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.sync_state
    ADD CONSTRAINT sync_state_pkey PRIMARY KEY (job_name);


--
-- Name: tip_reports tip_reports_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.tip_reports
    ADD CONSTRAINT tip_reports_pkey PRIMARY KEY (id);


--
-- Name: trading_signals trading_signals_date_unique; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.trading_signals
    ADD CONSTRAINT trading_signals_date_unique UNIQUE (signal_date);


--
-- Name: trading_signals trading_signals_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.trading_signals
    ADD CONSTRAINT trading_signals_pkey PRIMARY KEY (id);


--
-- Name: transaction_inputs transaction_inputs_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_inputs
    ADD CONSTRAINT transaction_inputs_pkey PRIMARY KEY (txid, vout_index);


--
-- Name: transaction_outputs transaction_outputs_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_outputs
    ADD CONSTRAINT transaction_outputs_pkey PRIMARY KEY (txid, vout_index);


--
-- Name: transactions transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_pkey PRIMARY KEY (txid);


--
-- Name: turnstile_daily turnstile_daily_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.turnstile_daily
    ADD CONSTRAINT turnstile_daily_pkey PRIMARY KEY (date, pool);


--
-- Name: zec_price_daily zec_price_daily_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.zec_price_daily
    ADD CONSTRAINT zec_price_daily_pkey PRIMARY KEY (date);


--
-- Name: idx_addr_tx_by_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_addr_tx_by_txid ON public.address_transactions USING btree (txid);


--
-- Name: idx_address_labels_category; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_address_labels_category ON public.address_labels USING btree (category);


--
-- Name: idx_blocks_timestamp_height_miner; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_blocks_timestamp_height_miner ON public.blocks USING btree ("timestamp", height, miner_address);


--
-- Name: idx_ccs_created_at; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_ccs_created_at ON public.cross_chain_swaps USING btree (swap_created_at DESC);


--
-- Name: idx_ccs_direction; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_ccs_direction ON public.cross_chain_swaps USING btree (direction);


--
-- Name: idx_ccs_unmatched; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_ccs_unmatched ON public.cross_chain_swaps USING btree (matched, match_attempts) WHERE (matched = false);


--
-- Name: idx_ccs_zec_address; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_ccs_zec_address ON public.cross_chain_swaps USING btree (zec_address) WHERE (zec_address IS NOT NULL);


--
-- Name: idx_ccs_zec_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_ccs_zec_txid ON public.cross_chain_swaps USING btree (zec_txid) WHERE (zec_txid IS NOT NULL);


--
-- Name: idx_chain_snapshots_time; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_chain_snapshots_time ON public.chain_snapshots USING btree (snapshot_time DESC);


--
-- Name: idx_fork_events_detected; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_fork_events_detected ON public.fork_events USING btree (detected_at DESC);


--
-- Name: idx_fork_events_height; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_fork_events_height ON public.fork_events USING btree (fork_height DESC);


--
-- Name: idx_fork_monitor_nodes_reported_at; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_fork_monitor_nodes_reported_at ON public.fork_monitor_nodes USING btree (reported_at);


--
-- Name: idx_mining_behavior_date; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_mining_behavior_date ON public.mining_behavior_daily USING btree (date);


--
-- Name: idx_node_snapshots_time; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_node_snapshots_time ON public.node_snapshots USING btree (snapshot_time DESC);


--
-- Name: idx_nodes_active_client; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_nodes_active_client ON public.nodes USING btree (observed_via, client_impl, client_version) WHERE (is_active = true);


--
-- Name: idx_nodes_active_location; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_nodes_active_location ON public.nodes USING btree (country_code, lat, lon) WHERE ((is_active = true) AND (lat IS NOT NULL) AND (lon IS NOT NULL));


--
-- Name: idx_nodes_last_seen; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_nodes_last_seen ON public.nodes USING btree (last_seen DESC);


--
-- Name: idx_orphaned_blocks_detected; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_blocks_detected ON public.orphaned_blocks USING btree (detected_at DESC);


--
-- Name: idx_orphaned_blocks_fork_event; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_blocks_fork_event ON public.orphaned_blocks USING btree (fork_event_id);


--
-- Name: idx_orphaned_blocks_height; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_blocks_height ON public.orphaned_blocks USING btree (height DESC);


--
-- Name: idx_orphaned_tx_block; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_tx_block ON public.orphaned_transactions USING btree (block_height DESC, tx_index);


--
-- Name: idx_orphaned_tx_block_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_tx_block_hash ON public.orphaned_transactions USING btree (block_hash);


--
-- Name: idx_orphaned_tx_coinbase; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_tx_coinbase ON public.orphaned_transactions USING btree (block_height) WHERE (is_coinbase = true);


--
-- Name: idx_orphaned_tx_fork_event; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_tx_fork_event ON public.orphaned_transactions USING btree (fork_event_id);


--
-- Name: idx_orphaned_txin_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_txin_txid ON public.orphaned_transaction_inputs USING btree (txid, block_hash);


--
-- Name: idx_orphaned_txout_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_orphaned_txout_txid ON public.orphaned_transaction_outputs USING btree (txid, block_hash);


--
-- Name: idx_patterns_deshield_txids; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_patterns_deshield_txids ON public.detected_patterns USING gin (deshield_txids);


--
-- Name: idx_patterns_detected_at; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_patterns_detected_at ON public.detected_patterns USING btree (detected_at DESC);


--
-- Name: idx_patterns_expires; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_patterns_expires ON public.detected_patterns USING btree (expires_at);


--
-- Name: idx_patterns_first_time; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_patterns_first_time ON public.detected_patterns USING btree (first_tx_time DESC);


--
-- Name: idx_patterns_score; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_patterns_score ON public.detected_patterns USING btree (score DESC);


--
-- Name: idx_patterns_shield_txids; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_patterns_shield_txids ON public.detected_patterns USING gin (shield_txids);


--
-- Name: idx_patterns_type; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_patterns_type ON public.detected_patterns USING btree (pattern_type);


--
-- Name: idx_patterns_warning; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_patterns_warning ON public.detected_patterns USING btree (warning_level);


--
-- Name: idx_privacy_batch_anchor_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_batch_anchor_txid ON public.privacy_batch_clusters USING btree (anchor_txid) WHERE (anchor_txid IS NOT NULL);


--
-- Name: idx_privacy_batch_evidence; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_batch_evidence ON public.privacy_batch_clusters USING gin (evidence);


--
-- Name: idx_privacy_batch_expires; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_batch_expires ON public.privacy_batch_clusters USING btree (expires_at);


--
-- Name: idx_privacy_batch_first_time; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_batch_first_time ON public.privacy_batch_clusters USING btree (first_tx_time DESC);


--
-- Name: idx_privacy_batch_member_txids; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_batch_member_txids ON public.privacy_batch_clusters USING gin (member_txids);


--
-- Name: idx_privacy_batch_score; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_batch_score ON public.privacy_batch_clusters USING btree (confidence_score DESC, first_tx_time DESC);


--
-- Name: idx_privacy_batch_warning; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_batch_warning ON public.privacy_batch_clusters USING btree (warning_level, first_tx_time DESC);


--
-- Name: idx_privacy_linkage_anchor_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_anchor_txid ON public.privacy_linkage_edges USING btree (anchor_txid) WHERE (anchor_txid IS NOT NULL);


--
-- Name: idx_privacy_linkage_detected_at; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_detected_at ON public.privacy_linkage_edges USING btree (detected_at DESC);


--
-- Name: idx_privacy_linkage_dst_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_dst_txid ON public.privacy_linkage_edges USING btree (dst_txid);


--
-- Name: idx_privacy_linkage_evidence; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_evidence ON public.privacy_linkage_edges USING gin (evidence);


--
-- Name: idx_privacy_linkage_expires; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_expires ON public.privacy_linkage_edges USING btree (expires_at);


--
-- Name: idx_privacy_linkage_rank; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_rank ON public.privacy_linkage_edges USING btree (edge_type, candidate_rank, dst_block_time DESC);


--
-- Name: idx_privacy_linkage_score; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_score ON public.privacy_linkage_edges USING btree (confidence_score DESC, dst_block_time DESC);


--
-- Name: idx_privacy_linkage_src_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_src_txid ON public.privacy_linkage_edges USING btree (src_txid);


--
-- Name: idx_privacy_linkage_warning; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_linkage_warning ON public.privacy_linkage_edges USING btree (warning_level, dst_block_time DESC);


--
-- Name: idx_privacy_stats_updated_at; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_stats_updated_at ON public.privacy_stats USING btree (updated_at DESC);


--
-- Name: idx_privacy_trends_date; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_trends_date ON public.privacy_trends_daily USING btree (date DESC);


--
-- Name: idx_sasd_chain_token; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_sasd_chain_token ON public.swap_amount_stats_daily USING btree (source_chain, source_token, date DESC);


--
-- Name: idx_shielded_flows_amount; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_amount ON public.shielded_flows USING btree (amount_zat);


--
-- Name: idx_shielded_flows_height; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_height ON public.shielded_flows USING btree (block_height);


--
-- Name: idx_shielded_flows_pool; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_pool ON public.shielded_flows USING btree (pool);


--
-- Name: idx_shielded_flows_time; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_time ON public.shielded_flows USING btree (block_time);


--
-- Name: idx_shielded_flows_type_amount_time; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_type_amount_time ON public.shielded_flows USING btree (flow_type, amount_zat, block_time);


--
-- Name: idx_tip_reports_height; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_tip_reports_height ON public.tip_reports USING btree (height DESC);


--
-- Name: idx_tip_reports_reported; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_tip_reports_reported ON public.tip_reports USING btree (reported_at DESC);


--
-- Name: idx_tip_reports_unique_report; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX idx_tip_reports_unique_report ON public.tip_reports USING btree (height, hash, COALESCE(node_id, ''::text));


--
-- Name: idx_trading_signals_date; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_trading_signals_date ON public.trading_signals USING btree (signal_date DESC);


--
-- Name: idx_transactions_block_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_block_hash ON public.transactions USING btree (block_hash);


--
-- Name: idx_transactions_block_time; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_block_time ON public.transactions USING btree (block_time);


--
-- Name: idx_transactions_coinbase_height_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_coinbase_height_txid ON public.transactions USING btree (block_height, txid) WHERE (is_coinbase = true);


--
-- Name: idx_transactions_height_index_txid; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_height_index_txid ON public.transactions USING btree (block_height DESC, tx_index DESC, txid);


--
-- Name: idx_transactions_ironwood_accounting; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_ironwood_accounting ON public.transactions USING btree (block_height) INCLUDE (value_balance_ironwood, value_balance_orchard, is_coinbase) WHERE (has_ironwood = true);


--
-- Name: idx_transactions_shielded_height_all; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_shielded_height_all ON public.transactions USING btree (block_height DESC) WHERE ((has_sapling = true) OR (has_orchard = true) OR (has_ironwood = true));


--
-- Name: idx_tx_inputs_address; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_inputs_address ON public.transaction_inputs USING btree (address) WHERE (address IS NOT NULL);


--
-- Name: idx_tx_inputs_prev_tx; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_inputs_prev_tx ON public.transaction_inputs USING btree (prev_txid, prev_vout) WHERE (prev_txid IS NOT NULL);


--
-- Name: idx_tx_outputs_address; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_outputs_address ON public.transaction_outputs USING btree (address) WHERE (address IS NOT NULL);


--
-- Name: mv_crosschain_latency_idx; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE UNIQUE INDEX mv_crosschain_latency_idx ON public.mv_crosschain_latency USING btree (direction, chain);


--
-- Name: mv_crosschain_popular_pairs_idx; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE UNIQUE INDEX mv_crosschain_popular_pairs_idx ON public.mv_crosschain_popular_pairs USING btree (chain, token);


--
-- Name: mv_crosschain_summary_idx; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE UNIQUE INDEX mv_crosschain_summary_idx ON public.mv_crosschain_summary USING btree (swaps_all_time);


--
-- Name: mv_crosschain_trends_idx; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE UNIQUE INDEX mv_crosschain_trends_idx ON public.mv_crosschain_trends USING btree (day, direction);


--
-- Name: mv_crosschain_volume_24h_idx; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE UNIQUE INDEX mv_crosschain_volume_24h_idx ON public.mv_crosschain_volume_24h USING btree (direction, chain, token);


--
-- Name: detected_patterns patterns_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER patterns_updated_at BEFORE UPDATE ON public.detected_patterns FOR EACH ROW EXECUTE FUNCTION public.update_patterns_timestamp();


--
-- Name: privacy_batch_clusters privacy_batch_clusters_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER privacy_batch_clusters_updated_at BEFORE UPDATE ON public.privacy_batch_clusters FOR EACH ROW EXECUTE FUNCTION public.update_privacy_linkage_timestamp();


--
-- Name: privacy_linkage_edges privacy_linkage_edges_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER privacy_linkage_edges_updated_at BEFORE UPDATE ON public.privacy_linkage_edges FOR EACH ROW EXECUTE FUNCTION public.update_privacy_linkage_timestamp();


--
-- Name: addresses trigger_update_address_timestamp; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER trigger_update_address_timestamp BEFORE UPDATE ON public.addresses FOR EACH ROW EXECUTE FUNCTION public.update_address_timestamp();


--
-- Name: orphaned_blocks orphaned_blocks_fork_event_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_blocks
    ADD CONSTRAINT orphaned_blocks_fork_event_id_fkey FOREIGN KEY (fork_event_id) REFERENCES public.fork_events(id) ON DELETE SET NULL;


--
-- Name: orphaned_transactions orphaned_transactions_fork_event_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.orphaned_transactions
    ADD CONSTRAINT orphaned_transactions_fork_event_id_fkey FOREIGN KEY (fork_event_id) REFERENCES public.fork_events(id) ON DELETE SET NULL;


--
-- Name: transaction_inputs transaction_inputs_txid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_inputs
    ADD CONSTRAINT transaction_inputs_txid_fkey FOREIGN KEY (txid) REFERENCES public.transactions(txid) ON DELETE CASCADE;


--
-- Name: transaction_outputs transaction_outputs_txid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_outputs
    ADD CONSTRAINT transaction_outputs_txid_fkey FOREIGN KEY (txid) REFERENCES public.transactions(txid) ON DELETE CASCADE;


--
-- Name: transactions transactions_block_height_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_block_height_fkey FOREIGN KEY (block_height) REFERENCES public.blocks(height) ON DELETE CASCADE;


--
-- Name: SCHEMA public; Type: ACL; Schema: -; Owner: pg_database_owner
--

GRANT ALL ON SCHEMA public TO zcash_user;


--
-- Name: TABLE address_transactions; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.address_transactions TO zcash_user;


--
-- Name: TABLE addresses; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.addresses TO zcash_user;


--
-- Name: TABLE blocks; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.blocks TO zcash_user;


--
-- Name: TABLE boundary_pool_snapshots; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.boundary_pool_snapshots TO zcash_user;


--
-- Name: TABLE cross_chain_swaps; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.cross_chain_swaps TO zcash_user;


--
-- Name: SEQUENCE cross_chain_swaps_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.cross_chain_swaps_id_seq TO zcash_user;


--
-- Name: TABLE detected_patterns; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.detected_patterns TO zcash_user;


--
-- Name: SEQUENCE detected_patterns_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.detected_patterns_id_seq TO zcash_user;


--
-- Name: TABLE fork_events; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.fork_events TO zcash_user;


--
-- Name: SEQUENCE fork_events_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.fork_events_id_seq TO zcash_user;


--
-- Name: TABLE fork_monitor_nodes; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.fork_monitor_nodes TO zcash_user;


--
-- Name: TABLE transactions; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.transactions TO zcash_user;


--
-- Name: TABLE node_snapshots; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.node_snapshots TO zcash_user;


--
-- Name: SEQUENCE node_snapshots_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.node_snapshots_id_seq TO zcash_user;


--
-- Name: TABLE nodes; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.nodes TO zcash_user;


--
-- Name: SEQUENCE nodes_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.nodes_id_seq TO zcash_user;


--
-- Name: TABLE orphaned_blocks; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.orphaned_blocks TO zcash_user;


--
-- Name: SEQUENCE orphaned_blocks_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.orphaned_blocks_id_seq TO zcash_user;


--
-- Name: TABLE orphaned_transaction_inputs; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.orphaned_transaction_inputs TO zcash_user;


--
-- Name: SEQUENCE orphaned_transaction_inputs_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.orphaned_transaction_inputs_id_seq TO zcash_user;


--
-- Name: TABLE orphaned_transaction_outputs; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.orphaned_transaction_outputs TO zcash_user;


--
-- Name: SEQUENCE orphaned_transaction_outputs_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.orphaned_transaction_outputs_id_seq TO zcash_user;


--
-- Name: TABLE orphaned_transactions; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.orphaned_transactions TO zcash_user;


--
-- Name: TABLE privacy_batch_clusters; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.privacy_batch_clusters TO zcash_user;


--
-- Name: SEQUENCE privacy_batch_clusters_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.privacy_batch_clusters_id_seq TO zcash_user;


--
-- Name: TABLE privacy_linkage_edges; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.privacy_linkage_edges TO zcash_user;


--
-- Name: SEQUENCE privacy_linkage_edges_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.privacy_linkage_edges_id_seq TO zcash_user;


--
-- Name: TABLE privacy_stats; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.privacy_stats TO zcash_user;


--
-- Name: SEQUENCE privacy_stats_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.privacy_stats_id_seq TO zcash_user;


--
-- Name: TABLE privacy_trends_daily; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.privacy_trends_daily TO zcash_user;


--
-- Name: SEQUENCE privacy_trends_daily_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.privacy_trends_daily_id_seq TO zcash_user;


--
-- Name: TABLE swap_amount_stats_daily; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.swap_amount_stats_daily TO zcash_user;


--
-- Name: TABLE sync_state; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.sync_state TO zcash_user;


--
-- Name: TABLE tip_reports; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.tip_reports TO zcash_user;


--
-- Name: SEQUENCE tip_reports_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.tip_reports_id_seq TO zcash_user;


--
-- Name: DEFAULT PRIVILEGES FOR SEQUENCES; Type: DEFAULT ACL; Schema: public; Owner: postgres
--

ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON SEQUENCES TO zcash_user;


--
-- Name: DEFAULT PRIVILEGES FOR TABLES; Type: DEFAULT ACL; Schema: public; Owner: postgres
--

ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON TABLES TO zcash_user;


--
-- PostgreSQL database dump complete
--


