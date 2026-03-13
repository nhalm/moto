# Architecture

This document describes Moto's system design, component interactions, and the philosophy behind its security model.

## Component Map

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                              User's Machine                                   │
│                                                                               │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  moto CLI                                                               │  │
│  │  - Garage management (open, close, list)                               │  │
│  │  - WireGuard client (userspace, no root required)                      │  │
│  │  - WebSocket client for logs/events                                    │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
└───────────────────────────────┬──────────────────────────────────────────────┘
                                │
                                │ HTTP/WebSocket (control plane)
                                │ WireGuard tunnel (terminal access)
                                │
                                v
┌──────────────────────────────────────────────────────────────────────────────┐
│                         Kubernetes Cluster (k3s/k3d)                          │
│                                                                               │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  moto-system namespace (control plane + shared services)               │  │
│  │                                                                         │  │
│  │  ┌──────────────────┐   ┌──────────────────┐   ┌─────────────────┐    │  │
│  │  │   moto-club      │   │    keybox        │   │   ai-proxy      │    │  │
│  │  │   (orchestrate)  │   │   (secrets)      │   │   (credentials) │    │  │
│  │  │                  │   │                  │   │                 │    │  │
│  │  │ - Garage CRUD    │   │ - SPIFFE auth    │   │ - Inject keys   │    │  │
│  │  │ - WireGuard      │   │ - Envelope enc   │   │ - Passthrough   │    │  │
│  │  │   coordination   │   │ - ABAC policies  │   │   routes        │    │  │
│  │  │ - K8s reconcile  │   │ - Audit logs     │   │ - Unified API   │    │  │
│  │  └────────┬─────────┘   └────────┬─────────┘   └────────┬────────┘    │  │
│  │           │                      │                       │             │  │
│  │           └──────────────────────┼───────────────────────┘             │  │
│  │                                  │                                     │  │
│  │  ┌───────────────────────────────┴────────────────────────┐            │  │
│  │  │   Postgres (control plane state)                       │            │  │
│  │  │   - Garages registry                                   │            │  │
│  │  │   - Secrets (encrypted)                                │            │  │
│  │  │   - Audit logs                                         │            │  │
│  │  └────────────────────────────────────────────────────────┘            │  │
│  └─────────────────────────────────────────────────────────────────────────┘  │
│                                                                               │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  moto-garage-{id} namespace (per-garage isolation)                     │  │
│  │                                                                         │  │
│  │  ┌──────────────────────────────────────────────────────────────────┐  │  │
│  │  │  Garage Pod                                                       │  │  │
│  │  │                                                                   │  │  │
│  │  │  ┌────────────────────────────────────────────────────────────┐  │  │  │
│  │  │  │  Dev Container (Nix-built, ~3GB)                           │  │  │  │
│  │  │  │  - Full Rust toolchain                                     │  │  │  │
│  │  │  │  - Claude Code or other AI agents                          │  │  │  │
│  │  │  │  - Root inside container (sandbox boundary)                │  │  │  │
│  │  │  │  - Read-only rootfs + writable volumes                     │  │  │  │
│  │  │  └────────────────────────────────────────────────────────────┘  │  │  │
│  │  │                                                                   │  │  │
│  │  │  ┌────────────────────────────────────────────────────────────┐  │  │  │
│  │  │  │  WireGuard Tunnel Daemon (moto-garage-wgtunnel)            │  │  │  │
│  │  │  │  - Userspace WireGuard (boringtun)                         │  │  │  │
│  │  │  │  - DERP client for NAT traversal                           │  │  │  │
│  │  │  │  - Ephemeral keypair (in-memory)                           │  │  │  │
│  │  │  └────────────────────────────────────────────────────────────┘  │  │  │
│  │  │                                                                   │  │  │
│  │  │  ┌────────────────────────────────────────────────────────────┐  │  │  │
│  │  │  │  Terminal Daemon (ttyd + tmux)                             │  │  │  │
│  │  │  │  - WebSocket terminal over WireGuard tunnel                │  │  │  │
│  │  │  │  - No auth (WireGuard tunnel IS the auth boundary)         │  │  │  │
│  │  │  └────────────────────────────────────────────────────────────┘  │  │  │
│  │  └──────────────────────────────────────────────────────────────────┘  │  │
│  │                                                                         │  │
│  │  NetworkPolicy: deny-all ingress, scoped egress                         │  │
│  │  SecurityContext: drop ALL capabilities, no privilege escalation        │  │
│  │                                                                         │  │
│  │  ┌─────────────────────────┐   ┌─────────────────────────┐             │  │
│  │  │ Postgres (per-garage)   │   │ Redis (per-garage)      │             │  │
│  │  │ - Ephemeral             │   │ - Ephemeral             │             │  │
│  │  └─────────────────────────┘   └─────────────────────────┘             │  │
│  └─────────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     │ (garages can reach internet)
                                     │
                                     v
                          ┌────────────────────┐
                          │  External Services │
                          │  - AI Providers    │
                          │  - Package repos   │
                          │  - Git hosts       │
                          └────────────────────┘
```

## Design Philosophy

### The Container is the Security Perimeter

Moto's security model starts with a simple premise: **the container boundary is the real sandbox**. AI agents run with full autonomy inside the garage—including root access—because the security guarantees come from container isolation (namespaces, cgroups, seccomp), not from restricting what the agent can do inside.

This design has several benefits:

- **No fake restrictions** — The agent has a real Linux environment with apt, cargo, nix, and all the tools it needs. No artificial limitations that break workflows.
- **Containment, not prevention** — We don't try to predict what an AI agent might do. Instead, we ensure that even if the agent does something unexpected, it can't escape the sandbox.
- **Fail closed** — If something goes wrong (compromised garage, runaway process), the blast radius is limited to that garage. Other garages, the control plane, and the host remain protected.

### Isolation by Default

Every garage runs in its own Kubernetes namespace with:

- **Deny-all ingress** — No incoming connections from anywhere (except WireGuard tunnel from the CLI)
- **Scoped egress** — Garages can reach the internet, keybox, ai-proxy, and their own supporting services (Postgres, Redis), but **cannot** reach moto-club, other garages, or the Kubernetes API
- **No K8s API access** — `automountServiceAccountToken: false` prevents garages from inspecting or modifying cluster state
- **ResourceQuota and LimitRange** — CPU, memory, and storage limits prevent resource exhaustion

This isolation means that a compromised or buggy garage is just as harmless as any other sandbox—it can't pivot to other workloads or exfiltrate data from shared state.

### Coordination, Not Relaying

moto-club is the orchestrator, but it **never sees traffic**. Two examples:

1. **WireGuard tunnels** — The CLI and garage establish a direct encrypted P2P connection (or DERP relay fallback). moto-club coordinates peer discovery and IP allocation, but terminal sessions flow end-to-end without touching the control plane.

2. **AI proxy** — Garages call ai-proxy directly (in-cluster HTTP). moto-club doesn't mediate requests, so it can't become a bottleneck or single point of failure for inference traffic.

This design reduces latency, improves scalability (moto-club doesn't need to handle high-throughput streaming), and limits the control plane's exposure to potentially sensitive data.

### Everything is Auditable

Every security-relevant operation generates an audit log:

- **Keybox** — Secret reads, writes, deletes, DEK rotations, SVID issuance
- **moto-club** — Garage creation, closure, owner changes
- **ai-proxy** — Provider requests (model name, token count, caller identity)

Audit logs are stored in Postgres with structured metadata (principal, resource, outcome, client IP) and are immutable—no updates, no deletes. This supports compliance requirements (SOC 2) and incident investigation.

## Data Flow: Creating a Garage

When a user runs `moto garage open`, here's what happens:

```
1. CLI sends request to moto-club
   POST /garages
   Body: { owner: "user@example.com", ttl_hours: 4 }

2. moto-club validates request and allocates resources
   - Generates garage ID (e.g., garage-abc123)
   - Allocates WireGuard IP (e.g., 10.42.1.5)
   - Creates database record (garages table)

3. moto-club creates Kubernetes resources
   - Namespace: moto-garage-abc123
   - NetworkPolicy: deny-all ingress, scoped egress
   - PersistentVolumeClaim: workspace storage
   - Pod: garage container with:
     * WireGuard tunnel daemon
     * Terminal daemon (ttyd + tmux)
     * Dev container (Nix tooling)
   - ConfigMap: WireGuard config (pushed by moto-club)

4. Garage pod starts and initializes
   - WireGuard daemon reads ConfigMap, generates ephemeral keypair
   - Registers peer with moto-club (POST /peers)
   - Terminal daemon starts listening (no auth - tunnel is boundary)

5. moto-club issues SPIFFE SVID for garage
   - Calls keybox: POST /auth/issue-garage-svid
   - SVID is Ed25519-signed JWT with:
     * spiffe_id: spiffe://moto.local/garage/{id}
     * pod_uid: {kubernetes pod UID}
     * ttl: 15 minutes (refreshed periodically)

6. Garage pod receives SVID
   - Injected as environment variable: MOTO_GARAGE_SVID
   - Used to authenticate to keybox and ai-proxy

7. CLI establishes WireGuard tunnel
   - Fetches peer info from moto-club (GET /garages/{id}/peer)
   - WireGuard handshake (direct UDP or DERP relay)
   - Routes 10.42.1.5 over encrypted tunnel

8. CLI connects to terminal
   - WebSocket connection to http://10.42.1.5:7681 (ttyd)
   - User sees interactive shell inside garage container
```

Once the garage is running, the agent can:

- Fetch secrets from keybox (using SVID for auth)
- Call AI providers through ai-proxy (using fake API key `garage-{id}`, which ai-proxy translates to real keys)
- Install packages, clone repos, build code—anything a developer would do

## The Motorcycle Metaphor

Moto uses motorcycle terminology to make the architecture intuitive. Here's the glossary:

| Term | Meaning | Technical Details |
|------|---------|-------------------|
| **The Club** | Central gathering place for riders | `moto-club` - the orchestrator service that manages all garages, coordinates WireGuard peers, and reconciles K8s state |
| **Garage** | Where you wrench on a bike | Isolated development environment (K8s namespace + pod) where AI agents work autonomously with full tooling |
| **Bike** | The finished product ready to ride | Production container (minimal, <20MB, no dev tools) that implements the "engine contract" and runs in production |
| **Keybox** | Secure storage locker | Secrets manager using SPIFFE identity, envelope encryption (AES-256 per secret), and ABAC policies |
| **Wrenching** | Working on a bike in the garage | Development work — prototyping, debugging, iterating with AI assistance in a sandboxed environment |
| **Ripping** | Riding the bike on the open road | Production deployment — running validated, containerized services with prod-grade security and monitoring |
| **SVID** | Your club membership card | SPIFFE Verifiable Identity Document - short-lived (15 min) Ed25519-signed JWT that proves identity to keybox and ai-proxy |
| **WireGuard Tunnel** | Private road to your garage | Encrypted P2P connection (or DERP relay) that gives you terminal access without exposing the garage to the internet |

**Why the metaphor works:**

- **Garages are sandboxes** — Just like a garage is separate from your house, each garage is isolated from other workloads
- **Bikes are minimal** — Production containers don't need wrenches, compilers, or dev tools—just the engine and wheels
- **The club coordinates but doesn't control** — moto-club helps riders find each other (WireGuard peers) but doesn't see what they're building
- **YOLO in the garage, cautious on the road** — Garages have full autonomy and root access; bikes run with least privilege

## Component Responsibilities

| Component | What it does | Where it runs |
|-----------|--------------|---------------|
| **moto-club** | Garage CRUD, WireGuard coordination, K8s reconciliation, registry of all garages (current and historical) | moto-system namespace (control plane) |
| **keybox** | Secrets storage and retrieval, SPIFFE identity verification, envelope encryption, audit logging | moto-system namespace (control plane) |
| **ai-proxy** | Reverse proxy for AI providers, credential injection, passthrough and unified endpoints, rate limiting | moto-system namespace (control plane) |
| **Garage pod** | Dev container + WireGuard tunnel + terminal daemon, isolated workspace for AI agents | Per-garage namespace (e.g., moto-garage-abc123) |
| **Supporting services** | Ephemeral Postgres and Redis for each garage, destroyed when garage closes | Per-garage namespace |
| **moto CLI** | User-facing commands (`garage open`, `dev up`), WireGuard client, WebSocket client for logs | User's machine |

## Network Topology

### What Garages Can Reach

✅ **Internet** — Package managers (crates.io, npmjs.com), Git hosts (github.com), docs, external APIs
✅ **keybox** — Fetch secrets using SVID authentication
✅ **ai-proxy** — Call AI providers with credential injection
✅ **Own supporting services** — Postgres and Redis in the same namespace
✅ **DNS** — Resolve hostnames via K8s CoreDNS

### What Garages CANNOT Reach

❌ **moto-club** — Control plane API is off-limits (prevents privilege escalation)
❌ **Other garages** — NetworkPolicy blocks cross-namespace traffic
❌ **Kubernetes API** — `automountServiceAccountToken: false` prevents API access
❌ **Cloud metadata endpoints** — Blocked by NetworkPolicy (prevents credential theft in cloud environments)
❌ **Host network** — Container network namespace isolation

This network topology ensures that even if a garage is compromised, the attacker is stuck inside a sandbox with no path to other workloads or sensitive infrastructure.

## Identity and Authentication

Moto uses SPIFFE-inspired identity for garages and bikes:

- **Garages** — Issued SVID by moto-club via keybox delegation. SVID contains `spiffe_id` (e.g., `spiffe://moto.local/garage/abc123`) and `pod_uid` (bound to Kubernetes pod).
- **Bikes** (future) — Authenticate using Kubernetes ServiceAccount JWT, which keybox validates via K8s TokenReview API.
- **Service tokens** — moto-club and other control plane services use pre-shared tokens stored in K8s Secrets.

**SVID properties:**

- **Short-lived** — 15-minute TTL, refreshed by garage pod
- **Cryptographically signed** — Ed25519 signature verified by keybox
- **Pod-bound** — keybox validates `pod_uid` matches a living pod via K8s API, preventing token replay after pod termination

When a garage calls keybox or ai-proxy, it includes the SVID in the request. The service validates the signature and claims, then enforces ABAC policies (e.g., garages can read global and service-scoped secrets, but not admin secrets).

## Key Design Decisions

### Why Kubernetes?

- **Namespace isolation** — Built-in resource and network isolation per garage
- **NetworkPolicies** — Declarative firewall rules that enforce zero-trust network boundaries
- **seccomp and capabilities** — Container security without custom sandboxing code
- **Pod lifecycle** — Automatic restarts, health checks, resource limits

### Why WireGuard?

- **End-to-end encryption** — Traffic never flows through moto-club or the control plane
- **Userspace implementation** — No root/sudo required on user's machine
- **NAT traversal** — DERP relay fallback works across firewalls and restrictive networks
- **Low overhead** — Minimal latency and CPU cost compared to VPN or SSH tunnels

### Why Envelope Encryption?

- **Per-secret DEKs** — Compromising one secret doesn't expose others
- **Master KEK rotation** — Can re-encrypt all DEKs without re-encrypting all secret values
- **Standard practice** — Aligns with NIST guidelines and cloud KMS patterns (AWS KMS, GCP Cloud KMS)

### Why ABAC (not RBAC)?

- **Flexible policies** — Can match on multiple dimensions (identity, scope, resource type) without combinatorial explosion
- **Future-proof** — Easy to extend with new attributes (time-of-day, IP ranges, compliance tags)
- **Bike restrictions** — Bikes in production should only access their own service's secrets, not all secrets in the cluster

## Local Development vs Production

Moto is designed to work the same way locally and in production, with a few differences:

| Aspect | Local (`moto dev up`) | Production |
|--------|----------------------|------------|
| **Cluster** | k3d (Docker-based k3s) | Real k3s or k8s cluster |
| **Secrets** | Generated on first run, stored in `.dev/k8s-secrets/` | HSM/KMS-backed (AWS KMS, GCP Cloud KMS) |
| **TLS** | Optional (cluster-internal HTTP) | Required (cert-manager + Let's Encrypt) |
| **Postgres** | Single-instance StatefulSet | Multi-replica with backups |
| **RBAC** | Permissive (developer ergonomics) | Least privilege (SOC 2 compliance) |
| **Audit logs** | Local Postgres (7-day retention) | Forwarded to SIEM or log aggregator |

The `make deploy` path mirrors production—it sets up the full moto-system namespace with StatefulSet Postgres, Deployments for moto-club and keybox (3 replicas each) and ai-proxy (2 replicas), and uses K8s Secrets for credentials (generated via `.dev/k8s-secrets/`).

## Future Architecture

Features planned for later phases:

- **Bike deployment** — moto-club deploys production containers to the same cluster or remote clusters
- **TTL enforcement** — moto-cron reconciler automatically closes expired garages (currently manual)
- **WebSocket streaming** — Real-time logs, events, and TTL warnings (currently REST polling)
- **Multi-region DERP** — Geographic distribution for lower-latency WireGuard relay fallback
- **K8s TokenReview** — Validate bike ServiceAccount JWTs via K8s API (currently trusted directly)
- **Rate limiting** — moto-throttle middleware limits AI proxy requests per garage (prevents runaway costs)
- **Audit log forwarding** — Ship audit events to external SIEM (Splunk, Datadog, etc.)

These are documented in individual specs (e.g., `moto-cron.md`, `moto-club-websocket.md`) and will be implemented incrementally.
