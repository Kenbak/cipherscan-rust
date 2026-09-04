-- Migration 019: bind mutable fork-monitor registrations to an ownership
-- token. Only the SHA-256 digest is stored; the API returns the random token
-- once when a node name is first registered.

ALTER TABLE public.fork_monitor_nodes
ADD COLUMN IF NOT EXISTS owner_token_hash text;
