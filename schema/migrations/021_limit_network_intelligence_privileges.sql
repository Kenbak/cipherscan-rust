-- Migration 021: reduce network-intelligence privileges for the shared app role.
--
-- Migration 015 intentionally granted broad privileges while the Ziggurat
-- integration was brought online. The application only needs ordinary DML on
-- these tables and sequence access for inserts; ownership-level operations
-- such as TRUNCATE, REFERENCES, and TRIGGER remain with postgres.

REVOKE ALL PRIVILEGES ON TABLE public.node_edges FROM zcash_user;
REVOKE ALL PRIVILEGES ON TABLE public.node_metrics FROM zcash_user;
REVOKE ALL PRIVILEGES ON TABLE public.nodes_crawl FROM zcash_user;
REVOKE ALL PRIVILEGES ON SEQUENCE public.node_edges_id_seq FROM zcash_user;
REVOKE ALL PRIVILEGES ON SEQUENCE public.node_metrics_id_seq FROM zcash_user;
REVOKE ALL PRIVILEGES ON SEQUENCE public.nodes_crawl_id_seq FROM zcash_user;

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.node_edges TO zcash_user;
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.node_metrics TO zcash_user;
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.nodes_crawl TO zcash_user;
GRANT USAGE, SELECT ON SEQUENCE public.node_edges_id_seq TO zcash_user;
GRANT USAGE, SELECT ON SEQUENCE public.node_metrics_id_seq TO zcash_user;
GRANT USAGE, SELECT ON SEQUENCE public.nodes_crawl_id_seq TO zcash_user;
