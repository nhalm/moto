# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## moto-club.md bug-fix

(all items completed)

## container-system.md bug-fix

(all items completed)

## moto-bike.md bug-fix

(all items completed)

## keybox.md bug-fix

(all items completed)

## makefile.md bug-fix

(all items completed)

## testing.md bug-fix

(all items completed)

## container-system.md bug-fix (2)

(all items completed)

## garage-isolation.md bug-fix

(all items completed)

## garage-lifecycle.md bug-fix

(all items completed)

## moto-bike.md bug-fix (2)

(all items completed)

## service-deploy.md bug-fix

(all items completed)

## service-deploy.md bug-fix (2)

(all items completed)

## moto-cron.md v0.2

(all items completed)

## moto-cron.md v0.3

(all items completed)

## moto-club-websocket.md v0.2

(all items completed)

## moto-club-websocket.md v0.3

(all items completed)

## moto-wgtunnel.md v0.10

(all items completed)

## moto-cli.md v0.14

(all items completed)

## ai-proxy.md v0.2

(all items completed)

## ai-proxy.md v0.3

(all items completed)

## ai-proxy.md v0.4

(all items completed)

## ai-proxy.md v0.5

(all items completed)

## testing.md v0.7

- [ ] Revert `crates/moto-ai-proxy/tests/smoke_test.rs` (already deleted — confirm no leftover references in Cargo.toml or CI)
- [ ] Create `infra/smoke-test-ai-proxy.sh` following the keybox pattern: passthrough auth (200/401), path allowlist (403), unified endpoint routing (200/400), health endpoints (200), missing provider (503)
- [ ] Add `smoke-ai-proxy` Makefile target: port-forward ai-proxy, run `infra/smoke-test-ai-proxy.sh`, clean up

## makefile.md v0.20

- [ ] Add `smoke-ai-proxy` target to Makefile (port-forward ai-proxy service, run smoke test script, kill port-forward on exit)
