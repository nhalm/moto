# Moto Specifications

> The Garage: secure workspaces for AI-assisted development.

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
| [moto-cli](moto-cli.md) | Ripping | CLI commands, args, UX |
| [jj-workflow](jj-workflow.md) | Ripping | How code flows via jj from garage to main |
| [pre-commit](pre-commit.md) | Ripping | Git hooks for fast feedback to agents |
| [makefile](makefile.md) | Ripping | Makefile targets and conventions |
| [testing](testing.md) | Ripping | Test infrastructure, Docker Compose, integration tests |

## Phase 1: Infrastructure (The Garage)

| Spec | Status | Description |
|------|--------|-------------|
| [moto-club](moto-club.md) | Ripping | Central orchestration, garage management |
| [keybox](keybox.md) | Ripping | Secrets manager, SPIFFE-based identity |
| [dev-container](dev-container.md) | Ripping | Nix dockerTools container, tooling, environment |
| [container-system](container-system.md) | Ripping | Build pipeline, registry |
| [local-cluster](local-cluster.md) | Ripping | Local k3s cluster, moto cluster CLI |
| [garage-isolation](garage-isolation.md) | Ripping | Network policies, resource limits |
| [garage-lifecycle](garage-lifecycle.md) | Ripping | Full lifecycle with ttyd terminal, TTL |
| [moto-bike](moto-bike.md) | Ripping | Bike base image, engine contract |
| [supporting-services](supporting-services.md) | Ripping | Postgres, Redis deployment |
| [moto-wgtunnel](moto-wgtunnel.md) | Ripping | WireGuard tunnels for terminal access |
| [local-dev](local-dev.md) | Ripping | Local dev stack: cargo run + docker-compose |
| [service-deploy](service-deploy.md) | Ripping | K8s deployment of moto-club, keybox, postgres |

## Phase 2: Future

| Spec | Status | Description |
|------|--------|-------------|
| [moto-throttle](moto-throttle.md) | Bare Frame | Rate limiting middleware |
| [moto-cron](moto-cron.md) | Ripping | TTL enforcement in reconciler, scheduled cleanup |
| [moto-club-websocket](moto-club-websocket.md) | Ripping | WebSocket streaming for peers, logs, events |
| [ai-proxy](ai-proxy.md) | Ready to Rip | AI provider gateway, injects secrets |
| [audit-logging](audit-logging.md) | Bare Frame | Compliance and audit trails |
| [vault-storage](vault-storage.md) | Bare Frame | Encrypted storage layer |
| [tokenization-api](tokenization-api.md) | Bare Frame | Data tokenization |
| [key-management](key-management.md) | Bare Frame | Key lifecycle management |
| [compliance](compliance.md) | Bare Frame | PCI DSS and SOC 2 requirements |
| [proxy-architecture](proxy-architecture.md) | Bare Frame | Tokenization proxy layer |
| [route-configuration](route-configuration.md) | Bare Frame | Proxy route configuration |
| [token-format](token-format.md) | Bare Frame | Token format specification |

