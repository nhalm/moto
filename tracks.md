# Moto Implementation Tracking

## Remaining

| Spec | Version | Item | Status |
|------|---------|------|--------|
| moto-club.md | v1.1 | moto-club-db: PostgreSQL migrations (garages, wg_devices, wg_sessions, wg_garages, user_ssh_keys, derp_servers tables) | |
| moto-club.md | v1.1 | moto-club-db: wg_devices repository (CRUD operations for WireGuard devices) | |
| moto-club.md | v1.1 | moto-club-db: wg_sessions repository (CRUD operations for tunnel sessions) | |
| moto-club.md | v1.1 | moto-club-db: wg_garages repository (garage WireGuard registration) | |
| moto-club.md | v1.1 | moto-club-db: user_ssh_keys repository (SSH key storage) | |
| moto-club.md | v1.1 | moto-club-db: derp_servers repository (DERP server config) | |
| moto-club.md | v1.1 | moto-club-k8s: garage namespace creation (labels, ServiceAccount, NetworkPolicy, ResourceQuota) | |
| moto-club.md | v1.1 | moto-club-k8s: SSH keys Secret creation (authorized_keys mounted to garage pod) | |
| moto-club.md | v1.1 | moto-club-k8s: dev container pod deployment | |
| moto-club.md | v1.1 | moto-club-k8s: namespace deletion on garage close | |
| moto-club.md | v1.1 | moto-club-garage: wire up K8s operations in garage service (create namespace, deploy pod, inject SSH keys) | |
| moto-club.md | v1.1 | moto-club-api/garages.rs: integrate K8s namespace creation in create_garage | |
| moto-club.md | v1.1 | moto-club-api/garages.rs: integrate K8s namespace deletion in delete_garage | |
| moto-club.md | v1.1 | moto-club-api/wg.rs: wire up PostgreSQL storage for devices (replace in-memory) | |
| moto-club.md | v1.1 | moto-club-api/wg.rs: wire up PostgreSQL storage for sessions (replace in-memory) | |
| moto-club.md | v1.1 | moto-club-api/wg.rs: wire up PostgreSQL storage for garage WG registration (replace in-memory) | |
| moto-club.md | v1.1 | moto-club-api/wg.rs: wire up PostgreSQL storage for SSH keys (replace in-memory) | |
| moto-club.md | v1.1 | moto-club-api/wg.rs: implement K8s ServiceAccount token validation for garage endpoints | |
| moto-club.md | v1.1 | moto-club-api/wg.rs: get_garage_peers - return active sessions from session manager | |
| moto-club.md | v1.1 | moto-club-api/wg.rs: list_sessions - implement device_id query param filtering | |
| moto-club.md | v1.1 | moto-club-api: GET /api/v1/wg/garages/{garage_id} endpoint (garage WG registration retrieval) | |
| moto-club.md | v1.1 | moto-club-api: GET /api/v1/wg/derp-map endpoint | |
| moto-club.md | v1.1 | moto-club-api: GET /api/v1/users/ssh-keys endpoint (list user SSH keys) | |
| moto-club.md | v1.1 | moto-club-api: DELETE /api/v1/users/ssh-keys/{key_id} endpoint | |
| moto-club.md | v1.1 | moto-club-api: GET /api/v1/info endpoint (server info) | |
| moto-club.md | v1.1 | moto-club: DERP config file loading (/etc/moto-club/derp.toml or MOTO_CLUB_DERP_CONFIG) | |
| moto-club.md | v1.1 | moto-club: DERP health check loop (30s interval, mark unhealthy after 3 failures) | |
| container-system.md | v0.9 | CI workflow: .github/workflows/containers.yml | future |
| container-system.md | v0.9 | Image signing: cosign keyless signing in CI | future |
| container-system.md | v0.9 | SBOM generation: trivy SBOM + cosign attestation | future |

## Implemented

<!-- Items completed during this loop run. Bookkeeping agent will move these to tracks-history.md -->

| Spec | Version | Item |
|------|---------|------|

---

## Workflow

1. Pick first non-blocked, non-future item from Remaining
2. Read the spec file, implement it, verify with tests
3. Move the row from Remaining to Implemented
4. Commit changes

**If only blocked/future items remain:** Output `LOOP_COMPLETE: true`

## Notes

- **Blocked:** Skip. Dependency must reach "Ready to Rip" first.
- **Future:** Skip. Belongs to a later phase.
- **Version mismatch:** If spec version > table version, check changelog for new items.
