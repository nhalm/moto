# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## docs v0.2

- Fix `docs/getting-started.md` port override env vars (line 279): change `CLUB_PORT=28080 KEYBOX_PORT=29090 AI_PROXY_PORT=27070` to use actual env var names `MOTO_CLUB_BIND_ADDR`, `MOTO_KEYBOX_BIND_ADDR`, `MOTO_AI_PROXY_BIND_ADDR` with full address values — the documented vars don't exist and are silently ignored.
- Fix `docs/getting-started.md` in-cluster DNS hostnames (lines 176, 180): change `keybox.moto-system.svc.cluster.local` to `moto-keybox.moto-system.svc.cluster.local` and `ai-proxy.moto-system.svc.cluster.local` to `moto-ai-proxy.moto-system.svc.cluster.local` — K8s services are named `moto-keybox` and `moto-ai-proxy` per infra/k8s manifests.
- Fix `docs/ai-proxy.md` KEYBOX_URL default (line 153): change `http://keybox.moto-system:8080` to `http://moto-keybox.moto-system:8080` — must match actual K8s service name in `ai-proxy.yaml` line 89.
