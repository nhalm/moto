# Garage Local

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Last Updated | 2026-01-20 |

## Overview

Local garage operations using K8s directly. No server, no WebSocket - just CLI talking to K8s. This is the foundation before adding remote/server capabilities.

## Jobs to Be Done

- [x] Define scope (local only, no moto-club dependency)
- [x] Define types needed
- [x] Define K8s operations needed
- [x] Define CLI commands

## Specification

### Scope

This spec covers:
- `moto garage list` - Query K8s for garages
- `moto garage open` - Create namespace + pod
- `moto garage close` - Delete namespace

Out of scope (see garage-remote.md later):
- `moto garage enter` - Requires WebSocket
- `moto garage sync` - Requires jj integration
- TTL enforcement - Requires background service

### Types

**GarageState:**
```rust
enum GarageState {
    Pending,    // Namespace created, pod starting
    Running,    // Pod running
    Ready,      // Pod ready (health check passed)
    Terminated, // Cleaned up
}
```

**GarageInfo:**
```rust
struct GarageInfo {
    id: String,           // Short UUID
    name: String,         // Human-friendly name
    namespace: String,    // moto-garage-{id}
    state: GarageState,
    created_at: DateTime<Utc>,
}
```

### K8s Operations

All operations use labels for filtering:
- `moto.dev/type: garage`
- `moto.dev/garage-id: {id}`
- `moto.dev/garage-name: {name}`

**create_garage_namespace(id, name):**
1. Create namespace `moto-garage-{id}`
2. Apply labels

**delete_garage_namespace(id):**
1. Delete namespace (cascades to all resources)

**list_garage_namespaces():**
1. List namespaces with label `moto.dev/type=garage`
2. Return list of GarageInfo

### CLI Commands

**`moto garage list`:**
```
moto garage list

ID       NAME                 STATUS    AGE
abc123   feature-foo          Ready     15m
def456   bugfix-bar           Pending   30s
```

**`moto garage open`:**
```
moto garage open [--name NAME]

# Creates namespace, prints info
Garage created: abc123
  Name:      feature-foo
  Namespace: moto-garage-abc123
  Status:    Pending
```

**`moto garage close`:**
```
moto garage close <id>

# Deletes namespace
Garage abc123 closed.
```

### Dependencies

- `kube` crate for K8s API
- `k8s-openapi` for types
- Existing K8s cluster (k3s assumed)

### What's NOT Included

- Pod deployment (just namespace for now)
- WebSocket attachment
- TTL/auto-cleanup
- Supporting services (Postgres, Redis)

These come in later specs once the foundation works.
