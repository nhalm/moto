# Moto Specifications

> The Garage: secure workspaces for AI-assisted development.

## How Specs Work

Specs are **steering documents** - they define WHAT to build and WHY, not HOW to implement.

**Workflow:**
1. **Spec phase** - We work through a spec until it's right
2. **Loop phase** - `loop.sh` runs agents that implement the spec
3. **Tracking** - [specd_work_list.md](../specd_work_list.md) for remaining work, [specd_history.md](../specd_history.md) for completed items

**Agents have autonomy** on implementation. The spec steers direction, the agent decides the code.

**Status is human-controlled.** Agents NEVER change spec status. Only humans move specs between states (Bare Frame → Wrenching → Ready to Rip → Ripping).

**specd_work_list.md:** Contains remaining work items. **specd_history.md:** Archive of completed work. See AGENTS.md for details.

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
| [dev-container](dev-container.md) | Ripping | Docker container, tooling, environment |
| [container-system](container-system.md) | Ripping | Build pipeline, registry |
| [local-cluster](local-cluster.md) | Ripping | Local k3s cluster, moto cluster CLI |
| [garage-isolation](garage-isolation.md) | Ripping | Network policies, resource limits |
| [garage-lifecycle](garage-lifecycle.md) | Ripping | Full lifecycle with ttyd terminal, TTL |
| [moto-bike](moto-bike.md) | Ripping | Bike base image, engine contract |
| [supporting-services](supporting-services.md) | Ripping | Postgres, Redis deployment |
| [moto-wgtunnel](moto-wgtunnel.md) | Ripping | WireGuard tunnels for terminal access |
| [local-dev](local-dev.md) | Ripping | Local dev stack: cargo run + docker-compose |
| [service-deploy](service-deploy.md) | Ripping | K8s deployment of moto-club, keybox, postgres |

## Cross-cutting

| Spec | Status | Description |
|------|--------|-------------|
| [docs](docs.md) | Ripping | Project README, docs/ folder, wiki publishing |
| [nix-removal](nix-removal.md) | Ready to Rip | Remove Nix, replace with standard Dockerfiles |

## Phase 2: Future

| Spec | Status | Description |
|------|--------|-------------|
| [moto-throttle](moto-throttle.md) | Ripping | Rate limiting middleware |
| [moto-cron](moto-cron.md) | Ripping | TTL enforcement in reconciler, scheduled cleanup |
| [moto-club-websocket](moto-club-websocket.md) | Ripping | WebSocket streaming for peers, logs, events |
| [ai-proxy](ai-proxy.md) | Ripping | AI provider gateway, injects secrets |
| [audit-logging](audit-logging.md) | Ripping | Compliance and audit trails |
| [compliance](compliance.md) | Ripping | SOC 2 compliance requirements and control mapping |

