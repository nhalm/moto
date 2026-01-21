# Moto Specifications

> Building a fintech motorcycle: tokenization, proxy, payments, lending.

## How Specs Work

Specs are **steering documents** - they define WHAT to build and WHY, not HOW to implement.

**Workflow:**
1. **Spec phase** - We work through a spec until it's right
2. **Loop phase** - `loop.sh` runs agents that implement the spec
3. **Tracks** - [tracks.md](../tracks.md) logs what's been implemented

**Agents have autonomy** on implementation. The spec steers direction, the agent decides the code.

**Changelog:** When a spec returns to "Ready to Rip" after being "Ripping", check the **Changelog** section at the bottom of the spec. It documents what changed between versions. Implement the delta, not the entire spec.

**Version tracking:** tracks.md entries include spec versions (e.g., `project-structure.md v1.0`). Compare the version in tracks.md against the current spec version. If the spec version is higher, check the changelog and implement the delta.

**Future items:** Items marked with `(future)` are for reference only. Do not implement them - they belong to a later phase or another spec.

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
| [moto-cli](moto-cli.md) | Bare Frame | CLI commands, args, UX |
| [jj-workflow](jj-workflow.md) | Bare Frame | How code flows via jj from garage to main |

## Phase 1: Infrastructure (The Garage)

| Spec | Status | Description |
|------|--------|-------------|
| [moto-club](moto-club.md) | Wrenching | Central orchestration, garage/bike management |
| [keybox](keybox.md) | Wrenching | Secrets manager, SPIFFE-based identity |
| [ai-proxy](ai-proxy.md) | Bare Frame | AI provider gateway, injects secrets |
| [dev-container](dev-container.md) | Wrenching | Tooling, environment, volumes |
| [container-system](container-system.md) | Wrenching | Build pipeline, registry |
| [k3s-cluster](k3s-cluster.md) | Bare Frame | Local cluster setup, persistence |
| [garage-isolation](garage-isolation.md) | Bare Frame | Network policies, resource limits |
| [garage-local](garage-local.md) | Wrenching | Local K8s operations (no server) |
| [garage-lifecycle](garage-lifecycle.md) | Wrenching | Full lifecycle with WebSocket, TTL |
| [bike](bike.md) | Wrenching | Runtime/deployment model |
| [supporting-services](supporting-services.md) | Bare Frame | Postgres, Redis deployment |

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
