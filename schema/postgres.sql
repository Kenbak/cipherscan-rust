--
-- PostgreSQL database dump
--

\restrict uLQclsQSby4Ozram9LoCtwYrBZJK6GOFVgQ81RD59frlfmRYzhhxbwdJ32cVGMb

-- Dumped from database version 14.20 (Ubuntu 14.20-0ubuntu0.22.04.1)
-- Dumped by pg_dump version 14.20 (Ubuntu 14.20-0ubuntu0.22.04.1)

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

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: address_clusters; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.address_clusters (
    id integer NOT NULL,
    cluster_id uuid NOT NULL,
    address text NOT NULL,
    confidence double precision DEFAULT 1.0,
    heuristic text,
    first_seen_txid text,
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamp with time zone DEFAULT now()
);


ALTER TABLE public.address_clusters OWNER TO zcash_user;

--
-- Name: address_clusters_id_seq; Type: SEQUENCE; Schema: public; Owner: zcash_user
--

CREATE SEQUENCE public.address_clusters_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.address_clusters_id_seq OWNER TO zcash_user;

--
-- Name: address_clusters_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: zcash_user
--

ALTER SEQUENCE public.address_clusters_id_seq OWNED BY public.address_clusters.id;


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
-- Name: address_relations; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.address_relations (
    id integer NOT NULL,
    address_a text NOT NULL,
    address_b text NOT NULL,
    relation_type text NOT NULL,
    confidence double precision DEFAULT 1.0 NOT NULL,
    txid text NOT NULL,
    block_height integer,
    created_at timestamp with time zone DEFAULT now()
);


ALTER TABLE public.address_relations OWNER TO zcash_user;

--
-- Name: address_relations_id_seq; Type: SEQUENCE; Schema: public; Owner: zcash_user
--

CREATE SEQUENCE public.address_relations_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.address_relations_id_seq OWNER TO zcash_user;

--
-- Name: address_relations_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: zcash_user
--

ALTER SEQUENCE public.address_relations_id_seq OWNED BY public.address_relations.id;


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
    next_block_hash text,
    total_fees bigint DEFAULT 0,
    miner_address text,
    created_at timestamp without time zone DEFAULT now(),
    confirmations integer
);


ALTER TABLE public.blocks OWNER TO postgres;

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
    CONSTRAINT detected_patterns_warning_level_check CHECK (((warning_level)::text = ANY ((ARRAY['HIGH'::character varying, 'MEDIUM'::character varying, 'LOW'::character varying])::text[])))
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


ALTER TABLE public.detected_patterns_id_seq OWNER TO postgres;

--
-- Name: detected_patterns_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.detected_patterns_id_seq OWNED BY public.detected_patterns.id;


--
-- Name: high_risk_patterns; Type: VIEW; Schema: public; Owner: postgres
--

CREATE VIEW public.high_risk_patterns AS
 SELECT detected_patterns.id,
    detected_patterns.pattern_type,
    detected_patterns.score,
    detected_patterns.warning_level,
    ((detected_patterns.total_amount_zat)::numeric / 100000000.0) AS total_amount_zec,
    ((detected_patterns.per_tx_amount_zat)::numeric / 100000000.0) AS per_tx_amount_zec,
    detected_patterns.batch_count,
    detected_patterns.time_span_hours,
    detected_patterns.shield_txids,
    detected_patterns.deshield_txids,
    (detected_patterns.metadata ->> 'explanation'::text) AS explanation,
    detected_patterns.detected_at
   FROM public.detected_patterns
  WHERE (((detected_patterns.warning_level)::text = 'HIGH'::text) AND (detected_patterns.expires_at > now()))
  ORDER BY detected_patterns.score DESC, detected_patterns.detected_at DESC;


ALTER TABLE public.high_risk_patterns OWNER TO postgres;

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
-- Name: mempool; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.mempool (
    txid text NOT NULL,
    size integer,
    fee bigint,
    time_added bigint,
    first_seen timestamp without time zone DEFAULT now()
);


ALTER TABLE public.mempool OWNER TO postgres;

--
-- Name: pattern_stats; Type: VIEW; Schema: public; Owner: postgres
--

CREATE VIEW public.pattern_stats AS
 SELECT detected_patterns.pattern_type,
    detected_patterns.warning_level,
    count(*) AS count,
    avg(detected_patterns.score) AS avg_score,
    (sum(detected_patterns.total_amount_zat) / 100000000.0) AS total_zec_flagged
   FROM public.detected_patterns
  WHERE (detected_patterns.expires_at > now())
  GROUP BY detected_patterns.pattern_type, detected_patterns.warning_level
  ORDER BY detected_patterns.pattern_type, detected_patterns.warning_level;


ALTER TABLE public.pattern_stats OWNER TO postgres;

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
    CONSTRAINT shielded_flows_pool_check CHECK ((pool = ANY (ARRAY['sapling'::text, 'orchard'::text, 'sprout'::text, 'mixed'::text])))
);


ALTER TABLE public.shielded_flows OWNER TO zcash_user;

--
-- Name: potential_roundtrips; Type: VIEW; Schema: public; Owner: postgres
--

CREATE VIEW public.potential_roundtrips AS
 SELECT s.txid AS shield_txid,
    d.txid AS deshield_txid,
    ((s.amount_zat)::numeric / 100000000.0) AS shield_amount,
    ((d.amount_zat)::numeric / 100000000.0) AS deshield_amount,
    ((abs((s.amount_zat - d.amount_zat)))::numeric / 100000000.0) AS difference,
    (((d.block_time - s.block_time))::numeric / 3600.0) AS hours_between,
    s.pool AS shield_pool,
    d.pool AS deshield_pool
   FROM (public.shielded_flows s
     JOIN public.shielded_flows d ON (((d.flow_type = 'deshield'::text) AND (s.flow_type = 'shield'::text) AND (d.block_time > s.block_time) AND (((d.amount_zat)::numeric >= ((s.amount_zat)::numeric * 0.99)) AND ((d.amount_zat)::numeric <= ((s.amount_zat)::numeric * 1.01))) AND ((d.block_time - s.block_time) < ((30 * 24) * 3600)))))
  ORDER BY (((d.block_time - s.block_time))::numeric / 3600.0)
 LIMIT 1000;


ALTER TABLE public.potential_roundtrips OWNER TO postgres;

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
    chain_supply bigint DEFAULT 0
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


ALTER TABLE public.privacy_stats_id_seq OWNER TO postgres;

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
    privacy_score integer DEFAULT 0
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


ALTER TABLE public.privacy_trends_daily_id_seq OWNER TO postgres;

--
-- Name: privacy_trends_daily_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.privacy_trends_daily_id_seq OWNED BY public.privacy_trends_daily.id;


--
-- Name: recent_blocks; Type: VIEW; Schema: public; Owner: postgres
--

CREATE VIEW public.recent_blocks AS
 SELECT blocks.height,
    blocks.hash,
    blocks."timestamp",
    blocks.transaction_count,
    blocks.size,
    blocks.difficulty
   FROM public.blocks
  ORDER BY blocks.height DESC
 LIMIT 50;


ALTER TABLE public.recent_blocks OWNER TO postgres;

--
-- Name: recent_deshields; Type: VIEW; Schema: public; Owner: postgres
--

CREATE VIEW public.recent_deshields AS
 SELECT shielded_flows.txid,
    shielded_flows.block_height,
    to_timestamp((shielded_flows.block_time)::double precision) AS "time",
    ((shielded_flows.amount_zat)::numeric / 100000000.0) AS amount_zec,
    shielded_flows.pool
   FROM public.shielded_flows
  WHERE (shielded_flows.flow_type = 'deshield'::text)
  ORDER BY shielded_flows.block_time DESC
 LIMIT 100;


ALTER TABLE public.recent_deshields OWNER TO postgres;

--
-- Name: recent_shields; Type: VIEW; Schema: public; Owner: postgres
--

CREATE VIEW public.recent_shields AS
 SELECT shielded_flows.txid,
    shielded_flows.block_height,
    to_timestamp((shielded_flows.block_time)::double precision) AS "time",
    ((shielded_flows.amount_zat)::numeric / 100000000.0) AS amount_zec,
    shielded_flows.pool
   FROM public.shielded_flows
  WHERE (shielded_flows.flow_type = 'shield'::text)
  ORDER BY shielded_flows.block_time DESC
 LIMIT 100;


ALTER TABLE public.recent_shields OWNER TO postgres;

--
-- Name: rich_list; Type: VIEW; Schema: public; Owner: postgres
--

CREATE VIEW public.rich_list AS
 SELECT addresses.address,
    addresses.balance,
    addresses.tx_count,
    addresses.address_type,
    addresses.last_seen
   FROM public.addresses
  WHERE (addresses.address_type = 'transparent'::text)
  ORDER BY addresses.balance DESC
 LIMIT 100;


ALTER TABLE public.rich_list OWNER TO postgres;

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


ALTER TABLE public.shielded_flows_id_seq OWNER TO zcash_user;

--
-- Name: shielded_flows_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: zcash_user
--

ALTER SEQUENCE public.shielded_flows_id_seq OWNED BY public.shielded_flows.id;


--
-- Name: transaction_inputs; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.transaction_inputs (
    id bigint NOT NULL,
    txid text,
    vout_index integer,
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
-- Name: transaction_inputs_id_seq; Type: SEQUENCE; Schema: public; Owner: zcash_user
--

CREATE SEQUENCE public.transaction_inputs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.transaction_inputs_id_seq OWNER TO zcash_user;

--
-- Name: transaction_inputs_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: zcash_user
--

ALTER SEQUENCE public.transaction_inputs_id_seq OWNED BY public.transaction_inputs.id;


--
-- Name: transaction_outputs; Type: TABLE; Schema: public; Owner: zcash_user
--

CREATE TABLE public.transaction_outputs (
    id bigint NOT NULL,
    txid text,
    vout_index integer,
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
-- Name: transaction_outputs_id_seq; Type: SEQUENCE; Schema: public; Owner: zcash_user
--

CREATE SEQUENCE public.transaction_outputs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER TABLE public.transaction_outputs_id_seq OWNER TO zcash_user;

--
-- Name: transaction_outputs_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: zcash_user
--

ALTER SEQUENCE public.transaction_outputs_id_seq OWNED BY public.transaction_outputs.id;


--
-- Name: transactions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.transactions (
    txid text NOT NULL,
    block_height bigint,
    block_hash text,
    "timestamp" bigint,
    version integer,
    locktime bigint,
    size integer,
    fee bigint DEFAULT 0,
    total_input bigint DEFAULT 0,
    total_output bigint DEFAULT 0,
    shielded_spends integer DEFAULT 0,
    shielded_outputs integer DEFAULT 0,
    orchard_actions integer DEFAULT 0,
    value_balance bigint DEFAULT 0,
    value_balance_sapling bigint DEFAULT 0,
    value_balance_orchard bigint DEFAULT 0,
    binding_sig text,
    binding_sig_sapling text,
    has_shielded_data boolean DEFAULT false,
    is_coinbase boolean DEFAULT false,
    confirmations integer DEFAULT 0,
    created_at timestamp without time zone DEFAULT now(),
    block_time bigint,
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
    flow_type text
);


ALTER TABLE public.transactions OWNER TO postgres;

--
-- Name: address_clusters id; Type: DEFAULT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.address_clusters ALTER COLUMN id SET DEFAULT nextval('public.address_clusters_id_seq'::regclass);


--
-- Name: address_relations id; Type: DEFAULT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.address_relations ALTER COLUMN id SET DEFAULT nextval('public.address_relations_id_seq'::regclass);


--
-- Name: detected_patterns id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.detected_patterns ALTER COLUMN id SET DEFAULT nextval('public.detected_patterns_id_seq'::regclass);


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
-- Name: transaction_inputs id; Type: DEFAULT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_inputs ALTER COLUMN id SET DEFAULT nextval('public.transaction_inputs_id_seq'::regclass);


--
-- Name: transaction_outputs id; Type: DEFAULT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_outputs ALTER COLUMN id SET DEFAULT nextval('public.transaction_outputs_id_seq'::regclass);


--
-- Name: address_clusters address_clusters_address_key; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.address_clusters
    ADD CONSTRAINT address_clusters_address_key UNIQUE (address);


--
-- Name: address_clusters address_clusters_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.address_clusters
    ADD CONSTRAINT address_clusters_pkey PRIMARY KEY (id);


--
-- Name: address_labels address_labels_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.address_labels
    ADD CONSTRAINT address_labels_pkey PRIMARY KEY (address);


--
-- Name: address_relations address_relations_address_a_address_b_txid_key; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.address_relations
    ADD CONSTRAINT address_relations_address_a_address_b_txid_key UNIQUE (address_a, address_b, txid);


--
-- Name: address_relations address_relations_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.address_relations
    ADD CONSTRAINT address_relations_pkey PRIMARY KEY (id);


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
-- Name: indexer_state indexer_state_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.indexer_state
    ADD CONSTRAINT indexer_state_pkey PRIMARY KEY (key);


--
-- Name: mempool mempool_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.mempool
    ADD CONSTRAINT mempool_pkey PRIMARY KEY (txid);


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
-- Name: shielded_flows shielded_flows_txid_flow_type_key; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.shielded_flows
    ADD CONSTRAINT shielded_flows_txid_flow_type_key UNIQUE (txid, flow_type);


--
-- Name: shielded_flows shielded_flows_txid_flow_unique; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.shielded_flows
    ADD CONSTRAINT shielded_flows_txid_flow_unique UNIQUE (txid, flow_type);


--
-- Name: transaction_inputs transaction_inputs_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_inputs
    ADD CONSTRAINT transaction_inputs_pkey PRIMARY KEY (id);


--
-- Name: transaction_inputs transaction_inputs_txid_vout_unique; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_inputs
    ADD CONSTRAINT transaction_inputs_txid_vout_unique UNIQUE (txid, vout_index);


--
-- Name: transaction_outputs transaction_outputs_pkey; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_outputs
    ADD CONSTRAINT transaction_outputs_pkey PRIMARY KEY (id);


--
-- Name: transaction_outputs transaction_outputs_txid_vout_unique; Type: CONSTRAINT; Schema: public; Owner: zcash_user
--

ALTER TABLE ONLY public.transaction_outputs
    ADD CONSTRAINT transaction_outputs_txid_vout_unique UNIQUE (txid, vout_index);


--
-- Name: transactions transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_pkey PRIMARY KEY (txid);


--
-- Name: idx_address_labels_category; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_address_labels_category ON public.address_labels USING btree (category);


--
-- Name: idx_addresses_balance; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_addresses_balance ON public.addresses USING btree (balance DESC);


--
-- Name: idx_addresses_last_seen; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_addresses_last_seen ON public.addresses USING btree (last_seen DESC);


--
-- Name: idx_addresses_tx_count; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_addresses_tx_count ON public.addresses USING btree (tx_count DESC);


--
-- Name: idx_addresses_type; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_addresses_type ON public.addresses USING btree (address_type);


--
-- Name: idx_blocks_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_blocks_hash ON public.blocks USING btree (hash);


--
-- Name: idx_blocks_previous_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_blocks_previous_hash ON public.blocks USING btree (previous_block_hash);


--
-- Name: idx_blocks_timestamp; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_blocks_timestamp ON public.blocks USING btree ("timestamp" DESC);


--
-- Name: idx_clusters_address; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_clusters_address ON public.address_clusters USING btree (address);


--
-- Name: idx_clusters_cluster_id; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_clusters_cluster_id ON public.address_clusters USING btree (cluster_id);


--
-- Name: idx_mempool_fee; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_mempool_fee ON public.mempool USING btree (fee DESC);


--
-- Name: idx_mempool_time; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_mempool_time ON public.mempool USING btree (time_added DESC);


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
-- Name: idx_privacy_stats_updated_at; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_stats_updated_at ON public.privacy_stats USING btree (updated_at DESC);


--
-- Name: idx_privacy_trends_date; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_privacy_trends_date ON public.privacy_trends_daily USING btree (date DESC);


--
-- Name: idx_relations_address_a; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_relations_address_a ON public.address_relations USING btree (address_a);


--
-- Name: idx_relations_address_b; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_relations_address_b ON public.address_relations USING btree (address_b);


--
-- Name: idx_shielded_flows_addresses; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_addresses ON public.shielded_flows USING gin (transparent_addresses);


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
-- Name: idx_shielded_flows_txid; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_txid ON public.shielded_flows USING btree (txid);


--
-- Name: idx_shielded_flows_type; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_type ON public.shielded_flows USING btree (flow_type);


--
-- Name: idx_shielded_flows_type_amount_time; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_shielded_flows_type_amount_time ON public.shielded_flows USING btree (flow_type, amount_zat, block_time);


--
-- Name: idx_transactions_block_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_block_hash ON public.transactions USING btree (block_hash);


--
-- Name: idx_transactions_block_height; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_block_height ON public.transactions USING btree (block_height DESC);


--
-- Name: idx_transactions_block_time; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_block_time ON public.transactions USING btree (block_time);


--
-- Name: idx_transactions_block_tx; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_block_tx ON public.transactions USING btree (block_height, tx_index);


--
-- Name: idx_transactions_coinbase; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_coinbase ON public.transactions USING btree (is_coinbase);


--
-- Name: idx_transactions_shielded; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_shielded ON public.transactions USING btree (has_shielded_data);


--
-- Name: idx_transactions_timestamp; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_transactions_timestamp ON public.transactions USING btree ("timestamp" DESC);


--
-- Name: idx_tx_flow_type; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX idx_tx_flow_type ON public.transactions USING btree (flow_type) WHERE (flow_type IS NOT NULL);


--
-- Name: idx_tx_inputs_address; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_inputs_address ON public.transaction_inputs USING btree (address);


--
-- Name: idx_tx_inputs_prev_tx; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_inputs_prev_tx ON public.transaction_inputs USING btree (prev_txid, prev_vout);


--
-- Name: idx_tx_inputs_txid; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_inputs_txid ON public.transaction_inputs USING btree (txid);


--
-- Name: idx_tx_outputs_address; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_outputs_address ON public.transaction_outputs USING btree (address);


--
-- Name: idx_tx_outputs_address_unspent; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_outputs_address_unspent ON public.transaction_outputs USING btree (address, spent) WHERE (spent = false);


--
-- Name: idx_tx_outputs_spent; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_outputs_spent ON public.transaction_outputs USING btree (spent);


--
-- Name: idx_tx_outputs_txid; Type: INDEX; Schema: public; Owner: zcash_user
--

CREATE INDEX idx_tx_outputs_txid ON public.transaction_outputs USING btree (txid);


--
-- Name: detected_patterns patterns_updated_at; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER patterns_updated_at BEFORE UPDATE ON public.detected_patterns FOR EACH ROW EXECUTE FUNCTION public.update_patterns_timestamp();


--
-- Name: addresses trigger_update_address_timestamp; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER trigger_update_address_timestamp BEFORE UPDATE ON public.addresses FOR EACH ROW EXECUTE FUNCTION public.update_address_timestamp();


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
-- Name: SCHEMA public; Type: ACL; Schema: -; Owner: postgres
--

GRANT ALL ON SCHEMA public TO zcash_user;


--
-- Name: TABLE addresses; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.addresses TO zcash_user;


--
-- Name: TABLE blocks; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.blocks TO zcash_user;


--
-- Name: TABLE detected_patterns; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.detected_patterns TO zcash_user;


--
-- Name: SEQUENCE detected_patterns_id_seq; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON SEQUENCE public.detected_patterns_id_seq TO zcash_user;


--
-- Name: TABLE high_risk_patterns; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.high_risk_patterns TO zcash_user;


--
-- Name: TABLE mempool; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.mempool TO zcash_user;


--
-- Name: TABLE pattern_stats; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.pattern_stats TO zcash_user;


--
-- Name: TABLE potential_roundtrips; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.potential_roundtrips TO zcash_user;


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
-- Name: TABLE recent_blocks; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.recent_blocks TO zcash_user;


--
-- Name: TABLE recent_deshields; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.recent_deshields TO zcash_user;


--
-- Name: TABLE recent_shields; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.recent_shields TO zcash_user;


--
-- Name: TABLE rich_list; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.rich_list TO zcash_user;


--
-- Name: TABLE transactions; Type: ACL; Schema: public; Owner: postgres
--

GRANT ALL ON TABLE public.transactions TO zcash_user;


--
-- Name: DEFAULT PRIVILEGES FOR SEQUENCES; Type: DEFAULT ACL; Schema: public; Owner: postgres
--

ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON SEQUENCES  TO zcash_user;


--
-- Name: DEFAULT PRIVILEGES FOR TABLES; Type: DEFAULT ACL; Schema: public; Owner: postgres
--

ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON TABLES  TO zcash_user;


--
-- Additive privacy linkage analytics objects
--

CREATE OR REPLACE FUNCTION public.update_privacy_linkage_timestamp() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$;


ALTER FUNCTION public.update_privacy_linkage_timestamp() OWNER TO postgres;

CREATE OR REPLACE FUNCTION public.cleanup_expired_privacy_linkage() RETURNS integer
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

CREATE TABLE IF NOT EXISTS public.privacy_linkage_edges (
    id integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    edge_hash character varying(64) NOT NULL UNIQUE,
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
    CONSTRAINT privacy_linkage_edges_edge_type_check CHECK (((edge_type)::text = ANY ((ARRAY['PAIR_LINK'::character varying, 'BATCH_LINK'::character varying])::text[]))),
    CONSTRAINT privacy_linkage_edges_warning_level_check CHECK (((warning_level)::text = ANY ((ARRAY['HIGH'::character varying, 'MEDIUM'::character varying, 'LOW'::character varying])::text[])))
);


ALTER TABLE public.privacy_linkage_edges OWNER TO postgres;

CREATE TABLE IF NOT EXISTS public.privacy_batch_clusters (
    id integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    cluster_hash character varying(64) NOT NULL UNIQUE,
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
    CONSTRAINT privacy_batch_clusters_cluster_type_check CHECK (((cluster_type)::text = ANY ((ARRAY['BATCH_DESHIELD'::character varying])::text[]))),
    CONSTRAINT privacy_batch_clusters_confidence_margin_check CHECK ((confidence_margin >= 0)),
    CONSTRAINT privacy_batch_clusters_confidence_score_check CHECK (((confidence_score >= 0) AND (confidence_score <= 100))),
    CONSTRAINT privacy_batch_clusters_member_count_check CHECK ((member_count >= 2)),
    CONSTRAINT privacy_batch_clusters_warning_level_check CHECK (((warning_level)::text = ANY ((ARRAY['HIGH'::character varying, 'MEDIUM'::character varying, 'LOW'::character varying])::text[])))
);


ALTER TABLE public.privacy_batch_clusters OWNER TO postgres;

CREATE INDEX IF NOT EXISTS idx_privacy_linkage_src_txid ON public.privacy_linkage_edges USING btree (src_txid);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_dst_txid ON public.privacy_linkage_edges USING btree (dst_txid);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_anchor_txid ON public.privacy_linkage_edges USING btree (anchor_txid) WHERE (anchor_txid IS NOT NULL);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_score ON public.privacy_linkage_edges USING btree (confidence_score DESC, dst_block_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_warning ON public.privacy_linkage_edges USING btree (warning_level, dst_block_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_rank ON public.privacy_linkage_edges USING btree (edge_type, candidate_rank, dst_block_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_detected_at ON public.privacy_linkage_edges USING btree (detected_at DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_expires ON public.privacy_linkage_edges USING btree (expires_at);
CREATE INDEX IF NOT EXISTS idx_privacy_linkage_evidence ON public.privacy_linkage_edges USING gin (evidence);

CREATE INDEX IF NOT EXISTS idx_privacy_batch_anchor_txid ON public.privacy_batch_clusters USING btree (anchor_txid) WHERE (anchor_txid IS NOT NULL);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_score ON public.privacy_batch_clusters USING btree (confidence_score DESC, first_tx_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_warning ON public.privacy_batch_clusters USING btree (warning_level, first_tx_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_first_time ON public.privacy_batch_clusters USING btree (first_tx_time DESC);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_expires ON public.privacy_batch_clusters USING btree (expires_at);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_member_txids ON public.privacy_batch_clusters USING gin (member_txids);
CREATE INDEX IF NOT EXISTS idx_privacy_batch_evidence ON public.privacy_batch_clusters USING gin (evidence);

DROP TRIGGER IF EXISTS privacy_linkage_edges_updated_at ON public.privacy_linkage_edges;
CREATE TRIGGER privacy_linkage_edges_updated_at BEFORE UPDATE ON public.privacy_linkage_edges FOR EACH ROW EXECUTE FUNCTION public.update_privacy_linkage_timestamp();

DROP TRIGGER IF EXISTS privacy_batch_clusters_updated_at ON public.privacy_batch_clusters;
CREATE TRIGGER privacy_batch_clusters_updated_at BEFORE UPDATE ON public.privacy_batch_clusters FOR EACH ROW EXECUTE FUNCTION public.update_privacy_linkage_timestamp();

CREATE OR REPLACE VIEW public.high_risk_privacy_linkage_edges AS
 SELECT privacy_linkage_edges.id,
    privacy_linkage_edges.edge_type,
    privacy_linkage_edges.src_txid,
    privacy_linkage_edges.dst_txid,
    privacy_linkage_edges.anchor_txid,
    ((privacy_linkage_edges.src_amount_zat)::numeric / 100000000.0) AS src_amount_zec,
    ((privacy_linkage_edges.dst_amount_zat)::numeric / 100000000.0) AS dst_amount_zec,
    privacy_linkage_edges.time_delta_seconds,
    privacy_linkage_edges.confidence_score,
    privacy_linkage_edges.confidence_margin,
    privacy_linkage_edges.ambiguity_score,
    privacy_linkage_edges.warning_level,
    privacy_linkage_edges.candidate_rank,
    privacy_linkage_edges.detected_at
   FROM public.privacy_linkage_edges
  WHERE ((privacy_linkage_edges.expires_at > now()) AND ((privacy_linkage_edges.warning_level)::text = 'HIGH'::text) AND (privacy_linkage_edges.candidate_rank = 1))
  ORDER BY privacy_linkage_edges.confidence_score DESC, privacy_linkage_edges.detected_at DESC;


ALTER TABLE public.high_risk_privacy_linkage_edges OWNER TO postgres;

CREATE OR REPLACE VIEW public.high_risk_privacy_batch_clusters AS
 SELECT privacy_batch_clusters.id,
    privacy_batch_clusters.cluster_type,
    privacy_batch_clusters.anchor_txid,
    ((privacy_batch_clusters.total_amount_zat)::numeric / 100000000.0) AS total_amount_zec,
    ((privacy_batch_clusters.representative_amount_zat)::numeric / 100000000.0) AS representative_amount_zec,
    privacy_batch_clusters.member_count,
    privacy_batch_clusters.first_tx_time,
    privacy_batch_clusters.last_tx_time,
    privacy_batch_clusters.time_span_seconds,
    privacy_batch_clusters.confidence_score,
    privacy_batch_clusters.confidence_margin,
    privacy_batch_clusters.ambiguity_score,
    privacy_batch_clusters.warning_level,
    privacy_batch_clusters.detected_at
   FROM public.privacy_batch_clusters
  WHERE ((privacy_batch_clusters.expires_at > now()) AND ((privacy_batch_clusters.warning_level)::text = 'HIGH'::text))
  ORDER BY privacy_batch_clusters.confidence_score DESC, privacy_batch_clusters.detected_at DESC;


ALTER TABLE public.high_risk_privacy_batch_clusters OWNER TO postgres;

GRANT ALL ON TABLE public.privacy_linkage_edges TO zcash_user;
GRANT ALL ON TABLE public.privacy_batch_clusters TO zcash_user;
GRANT ALL ON TABLE public.high_risk_privacy_linkage_edges TO zcash_user;
GRANT ALL ON TABLE public.high_risk_privacy_batch_clusters TO zcash_user;

--
-- PostgreSQL database dump complete
--

\unrestrict uLQclsQSby4Ozram9LoCtwYrBZJK6GOFVgQ81RD59frlfmRYzhhxbwdJ32cVGMb

