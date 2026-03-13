# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## docs v0.1

- Fix `docs/components.md` keybox anti-enumeration status code: change 404 to 403 (line 66) — code returns 403 for both "not found" and "access denied" (`moto-keybox/src/api.rs` line 621)
- Fix `docs/getting-started.md` registry port: change `localhost:5555` to `localhost:5050` (lines 73, 77, 86) — all other docs and k3d config use 5050
- Fix `docs/getting-started.md` ai-proxy in-cluster port: change `:7070` to `:8080` in curl example (line 180) — ai-proxy listens on 8080 per `bike.toml` and K8s manifest
- Fix `docs/security.md` keybox egress port: change 9090 to 8080 (lines 66, 207) — port 9090 is metrics, API is on 8080 per `keybox.yaml` Service definition
- Fix `docs/getting-started.md` keybox in-cluster port: change `:9090` to `:8080` in curl example (line 176) — keybox API is on port 8080, not 9090
- Fix `docs/deployment.md` ai-proxy replica count: change 3 to 2 (lines 34, 216) — `bike.toml` and K8s manifest specify `replicas: 2`
