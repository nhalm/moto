# Moto Agent Guidelines

## Specifications

**IMPORTANT** Before implementing any feature, consult the specifications in `specs/README.md`.

- **Assume NOT implemented.** Many specs describe planned features that may not yet exist in the codebase.
- **Check the codebase first.** Before concluding something is or isn't implemented, search the actual code. Specs describe intent; code describes reality.
- **Use specs as guidance.** When implementing a feature, follow the design patterns, types, and architecture defined in the relevant spec.
- **Spec index:** `specs/README.md` lists all specifications organized by phase.
- **Spec Changelogs do not change** when creating new changelog entries old entries are immutable.

## Motorcycle Metaphor

| Bike Part        | System Concept           | Description                                            |
| ---------------- | ------------------------ | ------------------------------------------------------ |
| **Club**         | Central orchestration    | The motorcycle club - manages garages and bikes        |
| **Garage**       | Wrenching environment    | Where you work on the bike (dev, Claude Code active)   |
| **Bike**         | Ripping runtime          | The bike on the road (production, serving traffic)     |
| **Engine**       | Business services        | What powers the bike - tokenization, payments, lending |
| **Frame**        | Infrastructure           | k3s, containers - structure holding it together        |
| **Transmission** | Proxy                    | Moves data between places                              |
| **Tank**         | Vault/storage            | Holds the valuable stuff (sensitive data)              |
| **Bars**         | CLI/control plane        | Handlebars - steering, directing the system            |
| **Chain**        | Data pipeline            | Connects engine to output                              |
| **Exhaust**      | Logging/audit            | What comes out, the trail                              |
| **Throttle**     | Rate limiting            | Controls speed                                         |
| **Brakes**       | Circuit breakers         | Safety cutoffs, emergency stops                        |
| **Mirrors**      | Monitoring/observability | Seeing what's happening around you                     |
| **Kickstand**    | Dev tooling              | Support when you're not running                        |
| **Switches**     | Feature flags            | Toggle functionality on/off                            |
| **Keybox**       | Secrets manager          | Where the keys live (SPIFFE-based)                     |
| **AI Proxy**     | AI provider gateway      | Injects secrets, routes to AI models                   |

## Crate Naming

All crates use the `moto-` prefix:

| Crate               | Purpose                                 |
| ------------------- | --------------------------------------- |
| `moto-cli`          | Binary: command dispatch (the Bars)     |
| `moto-club`         | Binary: central orchestration service   |
| `moto-club-api`     | Library: REST API handlers              |
| `moto-club-ws`      | Library: WebSocket handlers             |
| `moto-club-garage`  | Library: Garage service logic           |
| `moto-club-bike`    | Library: Bike service logic             |
| `moto-club-k8s`     | Library: Kubernetes interactions        |
| `moto-club-db`      | Library: Database layer                 |
| `moto-club-types`   | Library: Shared types (used by CLI too) |
| `moto-tank`         | Vault/encrypted storage                 |
| `moto-transmission` | Proxy layer                             |
| `moto-exhaust`      | Logging/audit                           |
| `moto-switches`     | Feature flags                           |
| `moto-throttle`     | Rate limiting                           |
| `moto-brakes`       | Circuit breakers, safety cutoffs        |
| `moto-mirrors`      | Monitoring, observability               |
| `moto-ai-proxy`     | AI provider proxy, secret injection     |
| `moto-keybox`       | Secrets manager (SPIFFE-based)          |

**Engines** (business services running inside bikes):

| Crate               | Purpose                    |
| ------------------- | -------------------------- |
| `moto-tokenization` | Tokenization core service  |
| `moto-payments`     | Payment processing service |
| `moto-lending`      | Lending/loans service      |

## Directory Structure

See [specs/project-structure.md](specs/project-structure.md) for the target directory layout.

## CLI Commands

**Garage commands (wrenching):**

```
moto garage open      # Create a new garage
moto garage enter     # Attach to a garage
moto garage detach    # Disconnect but keep alive (Ctrl+P, Ctrl+Q)
moto garage sync      # Sync code changes out via jj
moto garage list      # List all garages
moto garage logs      # View garage logs
moto garage extend    # Extend TTL
moto garage close     # Terminate and cleanup
```

**Bike commands (ripping):**

```
moto bike build       # Build a bike from the code
moto bike deploy      # Put a bike on the road (deploy to production)
moto bike list        # Show all running bikes
moto bike stop        # Take a bike off the road
moto bike logs        # See what a bike is doing
```
