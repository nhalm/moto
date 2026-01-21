# Garage Lifecycle

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Last Updated | 2026-01-19 |

## Overview

Defines the lifecycle operations for garages (wrenching environments). Covers creation, attachment, detachment, syncing, and cleanup. Supports multiple concurrent garages and remote execution via WebSocket.

## Jobs to Be Done

- [x] Define garage states
- [x] Define `moto garage open` behavior
- [x] Define `moto garage enter` behavior
- [x] Define `moto garage detach` behavior
- [x] Define `moto garage sync` behavior
- [x] Define `moto garage list` behavior
- [x] Define `moto garage close` behavior
- [x] Define TTL and auto-cleanup
- [x] Define WebSocket model for remote
- [ ] Define error handling and recovery
- [ ] Define resource limits per garage

## Specification

### Garage States

```
┌─────────┐     ┌─────────┐     ┌─────────┐     ┌──────────┐
│ Pending │ ──▶ │ Running │ ──▶ │  Ready  │ ──▶ │ Attached │
└─────────┘     └─────────┘     └─────────┘     └──────────┘
     │               │               │               │
     │               │               │               │
     ▼               ▼               ▼               ▼
┌─────────────────────────────────────────────────────────┐
│                      Terminated                         │
└─────────────────────────────────────────────────────────┘
```

| State | Description |
|-------|-------------|
| **Pending** | Pod scheduled, pulling images |
| **Running** | Container started, initializing |
| **Ready** | Garage ready, Claude Code available |
| **Attached** | User connected to garage |
| **Terminated** | Garage closed/cleaned up |

### CLI Commands

```
moto garage open [OPTIONS]      Create a new garage
moto garage enter <id>          Attach to a garage
moto garage detach              Disconnect but keep garage alive (Ctrl+P, Ctrl+Q)
moto garage sync <id>           Sync code changes out via jj
moto garage list                List all garages with status
moto garage close <id>          Terminate and cleanup garage
moto garage logs <id>           View garage logs
```

### `moto garage open`

Creates a new wrenching environment.

**Options:**
```
--name <name>           Human-friendly name (auto-generated if omitted)
--branch <branch>       Git branch to work on (default: current)
--ttl <duration>        Time-to-live (default: 4h, max: 48h)
--image <image>         Override dev container image
--no-attach             Create but don't attach
```

**Flow:**
```
1. Generate garage ID (UUID)
2. Create K8s namespace: moto-garage-{id}
3. Apply resource quotas and network policies
4. Deploy dev container pod
5. Clone repo / checkout branch inside container
6. Start supporting services (Postgres, Redis)
7. Wait for Ready state
8. If not --no-attach, attach via WebSocket
```

**Output:**
```
Garage created: abc123
  Name:    feature-tokenization
  Branch:  main
  TTL:     4h (expires 2026-01-20 02:48:00)
  Status:  Ready

Attaching... (Ctrl+P, Ctrl+Q to detach)
```

### `moto garage enter`

Attach to an existing garage.

**Flow:**
```
1. Lookup garage by ID or name
2. Verify garage is Ready or Attached
3. Establish WebSocket connection to garage pod
4. Relay stdin/stdout bidirectionally
5. Update state to Attached
```

**Detach:**
- `Ctrl+P, Ctrl+Q` detaches cleanly
- Garage keeps running
- Can reattach later

### `moto garage detach`

Disconnect from garage without stopping it.

- Keyboard shortcut: `Ctrl+P, Ctrl+Q`
- Garage continues running
- TTL timer continues
- Can reattach with `moto garage enter <id>`

### `moto garage sync`

Sync code changes from garage back to host.

**Flow:**
```
1. Inside garage: jj status to see changes
2. Squash/organize changes with jj
3. Push changes to a sync branch
4. On host: pull sync branch
5. Review and merge to working branch
```

**Options:**
```
--squash                Squash all changes into one commit
--message <msg>         Commit message for sync
```

### `moto garage list`

List all garages.

**Output:**
```
ID       NAME                    BRANCH   STATUS    TTL        AGE
abc123   feature-tokenization    main     Attached  3h 45m     15m
def456   fix-proxy-bug           develop  Ready     2h 10m     1h 50m
ghi789   experiment-caching      main     Pending   4h         30s
```

### `moto garage close`

Terminate and cleanup a garage.

**Flow:**
```
1. Warn if unsaved changes (prompt to sync first)
2. Delete dev container pod
3. Delete supporting service pods
4. Delete namespace
5. Remove from garage registry
```

**Options:**
```
--force                 Skip unsaved changes warning
```

### TTL and Auto-Cleanup

Garages have a time-to-live to prevent resource leaks.

| Setting | Value |
|---------|-------|
| Default TTL | 4 hours |
| Maximum TTL | 48 hours |
| Warning | 15 min before expiry |
| Grace period | 5 min after warning |

**Auto-cleanup flow:**
```
1. TTL timer starts at garage creation
2. At TTL - 15min: warn user (if attached)
3. At TTL - 5min: final warning, prompt to extend
4. At TTL: auto-close (with sync prompt if changes)
```

**Extend TTL:**
```
moto garage extend <id> --ttl 2h
```

### WebSocket Model

For remote garage execution, use WebSocket for stdin/stdout relay.

```
┌──────────────┐     WebSocket      ┌──────────────┐      exec      ┌──────────────┐
│   moto CLI   │ ◀───────────────▶ │  moto-server │ ◀────────────▶ │  Garage Pod  │
│   (local)    │     /ws/garage    │   (remote)   │    K8s exec    │   (remote)   │
└──────────────┘                    └──────────────┘                └──────────────┘
```

**WebSocket endpoints:**
```
/ws/garage/{id}/attach    Attach to garage (bidirectional)
/ws/garage/{id}/logs      Stream garage logs
```

**Protocol:**
- Binary frames for stdin/stdout
- JSON frames for control messages (resize, detach, etc.)
- Heartbeat every 30s to detect disconnection

### Multiple Garages

Multiple garages can run concurrently.

**Use cases:**
- Different features on different branches
- Experimenting with approaches in parallel
- One garage wrenching, others idle

**Constraints:**
- Each garage is independent (separate namespace)
- Resource limits per garage (CPU, memory)
- Total resource limit across all garages

### Garage Identity

Each garage gets a SPIFFE identity for keybox access:

```
spiffe://moto.local/garage/{garage-id}
```

Garage can fetch:
- Instance-scoped secrets (its own)
- Global secrets (AI keys via ai-proxy, etc.)
