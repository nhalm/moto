-- Add unique constraint on (region_id, host) for DERP servers
-- This enables upsert operations during config sync

CREATE UNIQUE INDEX IF NOT EXISTS idx_derp_servers_region_host ON derp_servers(region_id, host);
