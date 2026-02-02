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

## Phase 1: Infrastructure (The Garage)

| Spec | Status | Description |
|------|--------|-------------|
| [moto-club](moto-club.md) | Ripping | Central orchestration, garage management |
| [keybox](keybox.md) | Ripping | Secrets manager, SPIFFE-based identity |
| [ai-proxy](ai-proxy.md) | Bare Frame | AI provider gateway, injects secrets |
| [dev-container](dev-container.md) | Ripping | Nix dockerTools container, tooling, environment |
| [container-system](container-system.md) | Ripping | Build pipeline, registry |
| [local-cluster](local-cluster.md) | Ripping | Local k3s cluster, moto cluster CLI |
| [garage-isolation](garage-isolation.md) | Bare Frame | Network policies, resource limits |
| [garage-local](garage-local.md) | Wrenching | Local K8s operations (no server) |
| [garage-lifecycle](garage-lifecycle.md) | Ripping | Full lifecycle with ttyd terminal, TTL |
| [moto-bike](moto-bike.md) | Ripping | Bike base image, engine contract |
| [supporting-services](supporting-services.md) | Bare Frame | Postgres, Redis deployment |
| [moto-cron](moto-cron.md) | Bare Frame | Scheduled tasks (TTL cleanup), K8s CronJobs |
| [moto-wgtunnel](moto-wgtunnel.md) | Ripping | WireGuard tunnels for terminal access |
| [moto-club-websocket](moto-club-websocket.md) | Bare Frame | WebSocket streaming for logs/events |

