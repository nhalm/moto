-- Drop derp_servers table
-- Per moto-club.md v1.7: DERP config is now static per deployment via env var
-- No database table needed

DROP INDEX IF EXISTS idx_derp_servers_region_host;
DROP TABLE IF EXISTS derp_servers;
