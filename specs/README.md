# Moto Specifications

> Building a fintech motorcycle: tokenization, proxy, payments, lending.

## How Specs Work

Specs are **steering documents** - they define WHAT to build and WHY, not HOW to implement.

**Workflow:**
1. **Spec phase** - We work through a spec until it's right
2. **Loop phase** - `loop.sh` runs agents that implement the spec
3. **Tracks** - [tracks.md](../tracks.md) tracks Implemented vs Remaining for each spec

**Agents have autonomy** on implementation. The spec steers direction, the agent decides the code.

**Status is human-controlled.** Agents NEVER change spec status. Only humans move specs between states (Bare Frame → Wrenching → Ready to Rip → Ripping).

**tracks.md:** Tracks what's Implemented vs Remaining for each spec. See instructions in that file.

**Future items:** Items marked with `(future)` are for reference only. Do not implement them - they belong to a later phase or another spec.

**Dependencies:** If a feature depends on another spec, check that spec's status. Only implement if the dependency is Ready to Rip or Ripping. Mark blocked features in Remaining with "(blocked: specname.md)".

## Status Legend

| Status | Meaning |
|--------|---------|
| Bare Frame | Placeholder - needs spec work |
| Wrenching | Actively being specified |
| Ready to Rip | Spec complete, ready for implementation |
| Ripping | Fully implemented |

---

## Phase 0: Foundation

| Spec | Status | Description |
|------|--------|-------------|
| [project-structure](project-structure.md) | Ripping | Directory layout, crate organization, workspace |
| [moto-cli](moto-cli.md) | Ready to Rip | CLI commands, args, UX |
| [jj-workflow](jj-workflow.md) | Ripping | How code flows via jj from garage to main |

## Phase 1: Infrastructure (The Garage)

| Spec | Status | Description |
|------|--------|-------------|
| [moto-club](moto-club.md) | Ready to Rip | Central orchestration, garage management |
| [keybox](keybox.md) | Wrenching | Secrets manager, SPIFFE-based identity |
| [ai-proxy](ai-proxy.md) | Bare Frame | AI provider gateway, injects secrets |
| [dev-container](dev-container.md) | Ready to Rip | NixOS container, tooling, environment |
| [container-system](container-system.md) | Wrenching | Build pipeline, registry |
| [garage-isolation](garage-isolation.md) | Bare Frame | Network policies, resource limits |
| [garage-local](garage-local.md) | Wrenching | Local K8s operations (no server) |
| [garage-lifecycle](garage-lifecycle.md) | Wrenching | Full lifecycle with WebSocket, TTL |
| [bike](bike.md) | Wrenching | Runtime/deployment model |
| [supporting-services](supporting-services.md) | Bare Frame | Postgres, Redis deployment |
| [moto-cron](moto-cron.md) | Bare Frame | Scheduled tasks (TTL cleanup), K8s CronJobs |
| [moto-wgtunnel](moto-wgtunnel.md) | Ready to Rip | WireGuard tunnels for terminal/SSH access |
| [moto-club-websocket](moto-club-websocket.md) | Bare Frame | WebSocket streaming for logs/events |

## Phase 2: Tokenization (The Vault)

| Spec | Status | Description |
|------|--------|-------------|
| [token-format](token-format.md) | Bare Frame | Data types, token formats |
| [key-management](key-management.md) | Bare Frame | Key generation, rotation, storage |
| [vault-storage](vault-storage.md) | Bare Frame | Encrypted storage layer |
| [tokenization-api](tokenization-api.md) | Bare Frame | API for tokenize/detokenize |
| [compliance](compliance.md) | Bare Frame | PCI DSS, SOC 2, audit |

## Phase 3: Proxy (The Transmission)

| Spec | Status | Description |
|------|--------|-------------|
| [proxy-architecture](proxy-architecture.md) | Bare Frame | Overall proxy design |
| [route-configuration](route-configuration.md) | Bare Frame | Route matching, transformation |
| [audit-logging](audit-logging.md) | Bare Frame | What's logged, retention |

## Phase 4: Payments (Future)

_Specs to be defined_

## Phase 5: Lending (Future)

_Specs to be defined_
