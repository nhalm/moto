-- Initial moto-club database schema
-- See specs/moto-club.md for schema documentation

-- Garages (historical record, includes terminated)
CREATE TABLE garages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    owner TEXT NOT NULL,
    branch TEXT NOT NULL,
    status TEXT NOT NULL,           -- pending, running, ready, terminated
    image TEXT NOT NULL,            -- dev container image used
    ttl_seconds INTEGER NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    namespace TEXT NOT NULL,
    pod_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    terminated_at TIMESTAMPTZ,          -- NULL if not terminated, set when status = terminated
    termination_reason TEXT              -- NULL if not terminated; required when terminated
                                         -- Values: user_closed, ttl_expired, pod_lost, namespace_missing, error
);

CREATE INDEX idx_garages_owner ON garages(owner);
CREATE INDEX idx_garages_status ON garages(status);
CREATE INDEX idx_garages_expires_at ON garages(expires_at) WHERE status != 'terminated';
-- Note: idx_garages_name not needed; UNIQUE constraint on 'name' creates implicit index

-- WireGuard devices (client devices)
-- WireGuard public key IS the device identity (Cloudflare WARP model)
CREATE TABLE wg_devices (
    public_key TEXT PRIMARY KEY,    -- WG public key is the identifier
    owner TEXT NOT NULL,
    device_name TEXT,               -- optional friendly name
    assigned_ip TEXT NOT NULL,      -- fd00:moto:2::xxx
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_wg_devices_owner ON wg_devices(owner);

-- WireGuard sessions (active tunnel sessions)
CREATE TABLE wg_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_pubkey TEXT NOT NULL REFERENCES wg_devices(public_key),
    garage_id UUID NOT NULL REFERENCES garages(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at TIMESTAMPTZ
);

CREATE INDEX idx_wg_sessions_device ON wg_sessions(device_pubkey);
CREATE INDEX idx_wg_sessions_garage ON wg_sessions(garage_id);
CREATE INDEX idx_wg_sessions_expires ON wg_sessions(expires_at) WHERE closed_at IS NULL;

-- Garage WireGuard registration (set by garage pod on startup)
CREATE TABLE wg_garages (
    garage_id UUID PRIMARY KEY REFERENCES garages(id) ON DELETE CASCADE,
    public_key TEXT NOT NULL UNIQUE,
    assigned_ip TEXT NOT NULL,          -- fd00:moto:1::xxx
    endpoints TEXT[] NOT NULL,          -- pod's reachable endpoints
    peer_version INTEGER NOT NULL DEFAULT 0,  -- incremented on session create/close
    registered_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- DERP servers (monitored by moto-club)
CREATE TABLE derp_servers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    region_id INTEGER NOT NULL,
    region_name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 443,
    stun_port INTEGER NOT NULL DEFAULT 3478,
    healthy BOOLEAN NOT NULL DEFAULT true,
    last_check_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
