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

- Ensure TTL enforcement applies to all non-terminated states: Pending, Initializing, Ready, and Failed

## moto-club-websocket.md v0.2

- Implement log streaming WebSocket endpoint: /ws/v1/garages/{name}/logs with tail, follow, since query params
- Implement K8s pod log stream integration: historical lines first, then follow if requested, eof on pod terminate
- Implement event streaming WebSocket endpoint: /ws/v1/events with garages query param filter
- Implement TTL warning events in reconciler: emit ttl_warning at 15 and 5 minutes before expiry
- Implement status_change events on garage state transitions (from garage service and reconciler)
- Implement error events from reconciler (pod failures, crash loops)
- Update CLI to prefer WebSocket for log streaming, fall back to direct K8s API

## moto-club-websocket.md v0.3

- Add owner-based auth (same as REST API) to log and event streaming endpoints
- Add garage state validation for log streaming: reject Pending and Terminated, allow Initializing/Ready/Failed
- Add dropped message type for log backpressure: buffer up to 256 messages, drop oldest and notify client
- Add connection limits: max 5 concurrent log WS connections per garage, max 3 event WS connections per user
- Add reason field to status_change events on transitions to Terminated or Failed (values from TerminationReason enum)
