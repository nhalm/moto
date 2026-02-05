# Garage Lifecycle

| | |
|--------|----------------------------------------------|
| Version | 0.4 |
| Status | Ripping |
| Last Updated | 2026-02-04 |

## Overview

Defines the lifecycle operations for garages (wrenching environments). Covers creation, connection, and cleanup. Supports multiple concurrent garages and remote execution via WireGuard.

**Connectivity model:**

| Transport | Purpose | Details |
|-----------|---------|---------|
| **WireGuard** | Terminal access (ttyd) | Encrypted P2P tunnel, moto-club coordinates only |
| **WebSocket/SSE** | Streaming logs, TTL warnings, events | Future - see [moto-club-websocket.md](moto-club-websocket.md) |

See [moto-wgtunnel.md](moto-wgtunnel.md) for WireGuard spec.

## Specification

### Garage States

```
┌─────────┐     ┌──────────────┐     ┌─────────┐
│ Pending │ ──▶ │ Initializing │ ──▶ │  Ready  │
└─────────┘     └──────────────┘     └─────────┘
     │                 │                  │
     ▼                 ▼                  ▼
┌──────────┐   ┌───────────────────────────────┐
│  Failed  │   │          Terminated           │
└──────────┘   └───────────────────────────────┘
```

| State | Description |
|-------|-------------|
| **Pending** | Pod scheduled, pulling images |
| **Initializing** | Pod running, cloning repo, starting services |
| **Ready** | Garage ready for use (see Ready criteria below) |
| **Failed** | Startup failed (clone error, bad credentials, etc.) |
| **Terminated** | Garage closed/cleaned up |

**Failed state:** Garage transitions to Failed if:
- Init container fails (clone error, credentials invalid)
- Pod fails to start after timeout (image pull failure, resource limits)
- WireGuard registration fails

Failed garages can be inspected with `moto garage logs <name>` and cleaned up with `moto garage close <name>`.

### Ready Criteria

A garage transitions to Ready when ALL of the following are true:

| Check | Description |
|-------|-------------|
| Pod running | K8s pod status is Running |
| Terminal daemon up | ttyd accepting connections on port 7681 |
| WireGuard registered | Garage has registered with moto-club |
| Repo cloned | Repository cloned to `/workspace/<repo-name>/` |

### CLI Commands

```
moto garage open [OPTIONS]      Create a new garage
moto garage enter <name>        Connect to a garage terminal
moto garage list                List all garages with status
moto garage close <name>        Terminate and cleanup garage
moto garage extend <name>       Extend garage TTL
moto garage logs <name>         View garage logs
```

**Note:** Code sync and PR creation are handled by agents using `jj` and `gh` directly. See [jj-workflow.md](jj-workflow.md) for the workflow.

### `moto garage open`

Creates a new wrenching environment.

**Options:**
```
--name <name>           Human-friendly name (auto-generated if omitted)
--branch <branch>       Git branch to work on (default: current)
--ttl <duration>        Time-to-live (default: 4h, max: 48h)
--image <image>         Override dev container image
--no-attach             Create but don't connect
```

**Flow:**
```
1. Generate garage ID (UUID)
2. Create K8s namespace: moto-garage-{id}
3. Apply resource quotas and network policies
4. Deploy dev container pod with repo config (URL, branch) as env vars
5. Wait for pod Running
6. Init container clones repository (see Repository Cloning below)
7. Wait for Ready state (all criteria met)
8. If not --no-attach, connect via WireGuard tunnel
```

### Repository Cloning

Handled by an init container before the main garage container starts:

```
1. Init container starts with git credentials mounted from keybox
2. Clone: git clone --depth=1 <repo-url> /workspace/<repo-name>/
3. Checkout: git checkout -b <branch> origin/<branch> (or create new branch)
4. Set working directory marker for ttyd
5. Init container exits, main container starts
```

**Environment variables (injected by moto-club):**
- `REPO_URL` - Repository clone URL
- `REPO_BRANCH` - Branch to checkout
- `REPO_NAME` - Directory name under /workspace/

**Credentials:** Git credentials (token or SSH key) fetched from keybox, mounted as a Secret. Cleaned up when garage terminates.

**Failure handling:** If clone fails after 3 retries, init container fails and garage transitions to Failed state.

**Output:**
```
Garage created: abc123
  Name:    feature-tokenization
  Branch:  main
  TTL:     4h (expires 2026-01-20 02:48:00)
  Status:  Ready

Connecting... (use `moto garage enter <name>` to reconnect)
```

### `moto garage enter`

Connect to an existing garage's terminal.

**Flow:**
```
1. Lookup garage by name
2. Verify garage is Ready
3. Establish WireGuard tunnel (see moto-wgtunnel.md)
4. Connect to ttyd via WebSocket
5. Attach to tmux session
```

**Session behavior:**
- First connect → creates tmux session, attaches
- Disconnect → tmux session keeps running (processes survive)
- Reconnect → reattaches to existing tmux session
- Multiple clients → all attach to same tmux session (mirrored view)

**Detach:** Close the connection or use tmux detach (`Ctrl+B, D`). Garage keeps running.

**Authentication:** The WireGuard tunnel is the sole authentication boundary. If you can establish the tunnel (which requires a valid session from moto-club), you have full terminal access. No additional SSH keys or passwords needed.

See [moto-wgtunnel.md](moto-wgtunnel.md) for connection details.

### `moto garage list`

List all garages.

**Output:**
```
ID       NAME                    BRANCH   STATUS    TTL        AGE
abc123   feature-tokenization    main     Ready     3h 45m     15m
def456   fix-proxy-bug           develop  Ready     2h 10m     1h 50m
ghi789   experiment-caching      main     Pending   4h         30s
```

### `moto garage close`

Terminate and cleanup a garage.

**Flow:**
```
1. Warn if unsaved changes (prompt to sync first)
2. Delete K8s namespace (cascades to all resources)
3. Update database status to Terminated
```

**Options:**
```
--force                 Skip unsaved changes warning
```

### `moto garage extend`

Extend garage TTL.

```
moto garage extend <name> --ttl 2h
```

Adds time to current expiry. Cannot exceed max TTL (48h total).

### TTL and Cleanup

Garages have a time-to-live to prevent resource leaks.

| Setting | Value |
|---------|-------|
| Default TTL | 4 hours |
| Maximum TTL | 48 hours |

**Cleanup:** TTL enforcement handled by moto-cron. See [moto-cron.md](moto-cron.md). For now, use `moto garage close` to manually close garages.

### Connectivity Model

**Terminal access (WireGuard + ttyd):**
```
┌──────────────┐     WireGuard      ┌──────────────┐
│   moto CLI   │ ◀────────────────▶ │  Garage Pod  │
│   (local)    │   tunnel + ttyd    │   (remote)   │
└──────────────┘                    └──────────────┘
        │
        │  coordinate
        ▼
┌──────────────┐
│  moto-club   │  (peer registration, IP allocation only)
└──────────────┘
```

**Streaming (future):**
```
/ws/v1/garages/{name}/logs    Stream garage logs (WebSocket)
/ws/v1/events                 TTL warnings, status changes (WebSocket)
```

See [moto-wgtunnel.md](moto-wgtunnel.md) for WireGuard details.
See [moto-club-websocket.md](moto-club-websocket.md) for streaming details.

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

## Changelog

### v0.4
- Fix: Implement `--branch` flag in CLI (currently documented but not in CLI args)
- Fix: Implement `--no-attach` flag in CLI (currently documented but not implemented)
- Fix: Add "repo cloned" ready criteria check (verify init container completed successfully)
- Fix: `garage open` output format should match spec (show ID, branch, expires_at, status)
- Fix: `garage list` columns should match spec (add ID, BRANCH columns)

### v0.3
- Rename "Running" to "Initializing", add "Failed" state (5 states: Pending → Initializing → Ready/Failed → Terminated)
- Replace SSH with ttyd + tmux for terminal access
- Add Repository Cloning section (init container, credentials from keybox)
- Add Ready criteria section (pod running, ttyd up, WireGuard registered, repo cloned)
- Update connectivity model for ttyd
- Add repo cloning details (`/workspace/<repo-name>/`, credentials from keybox)
- Defer supporting services to future (not in garage open flow)
- Defer TTL enforcement to moto-cron
- CLI commands use `<name>` instead of `<id>`
- Add `garage extend` command

### v0.2
- Initial specification

## References

- [moto-wgtunnel.md](moto-wgtunnel.md) - WireGuard tunnel system
- [moto-club.md](moto-club.md) - Garage orchestration
- [moto-club-websocket.md](moto-club-websocket.md) - WebSocket streaming
- [moto-cron.md](moto-cron.md) - Scheduled tasks (TTL cleanup)
- [jj-workflow.md](jj-workflow.md) - Code workflow
