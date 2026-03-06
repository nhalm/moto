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

- [ ] Update health endpoint to reflect WebSocket connection status (`moto_club_connected`) and WireGuard tunnel status (`wireguard`)

## moto-cli.md v0.14

- [ ] Add `Watch` variant to `GarageAction` enum with `--garages` option (comma-separated names, optional)
- [ ] Add `stream_events_ws()` method to `MotoClubClient`: connect to `/ws/v1/events?garages=...` WebSocket, same auth pattern as `stream_logs_ws()`, return channel of parsed GarageEvent messages
- [ ] Implement `watch` command handler: connect via `stream_events_ws()`, format events for human output (e.g. `[garage-name] Status: From → To`), support `--json` for JSON Lines output (one event per line)
- [ ] Implement reconnect logic in watch: backoff (1s, 2s, 4s, cap 10s), fetch current state via REST on reconnect before resuming WebSocket
