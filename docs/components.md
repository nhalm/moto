# Components

This page provides a reference for all major components in the Moto system.

## Component Reference

| Component | Crate(s) | What it does |
|-----------|----------|--------------|
| moto-club | `moto-club-*` | Central orchestrator — manages garages, WireGuard peers, reconciliation loop |
| moto-cli | `moto-cli` | User-facing CLI — `moto garage open`, `moto dev up`, etc. |
| Keybox | `moto-keybox-*` | Secrets manager — SPIFFE identity, envelope encryption |
| AI Proxy | `moto-ai-proxy` | Credential-injecting reverse proxy for AI providers |
| Garage | `moto-garage` (image) | Dev container — Nix-built, ~3GB, full Rust toolchain + Claude Code |
| Bike | `moto-bike` (image) | Minimal production container (<20MB), engine contract |
| WireGuard Tunnel | `moto-wgtunnel` | Encrypted terminal access via userspace WireGuard + DERP relay |
| Throttle | `moto-throttle` | Rate limiting middleware (tower) |
| Cron | (in moto-club) | TTL enforcement and audit log retention in reconciler loop |
| Supporting Services | (K8s manifests) | Per-garage Postgres and Redis, ephemeral |

## moto-club

**Central orchestrator and control plane for the Moto system.**

moto-club is the heart of the garage management system. It runs as a multi-replica Kubernetes Deployment in the `moto-system` namespace and coordinates all garage lifecycle operations. When you run `moto garage open`, the CLI talks to moto-club, which:

- Creates an isolated namespace for the garage
- Generates a WireGuard keypair and registers the peer
- Provisions a SPIFFE SVID (short-lived identity token)
- Deploys the garage pod with NetworkPolicies and resource limits
- Optionally spins up supporting services (Postgres, Redis)

moto-club also runs a continuous reconciliation loop that enforces TTLs, cleans up expired garages, and ensures the cluster state matches desired state. It uses leader election to run safely with multiple replicas.

**Key design decisions:**
- **Never relays data** — moto-club coordinates, but garages communicate directly with keybox and ai-proxy. This keeps moto-club stateless and avoids it becoming a bottleneck or trust boundary.
- **Fail-closed** — If moto-club can't reach dependencies (keybox, Kubernetes API), it denies requests rather than degrading gracefully.

## moto-cli

**User-facing command-line interface.**

The CLI is the primary way users interact with Moto. It provides commands for:

- `moto garage open` — Create and connect to a new garage
- `moto garage list` — Show active garages
- `moto garage close <id>` — Terminate a garage
- `moto dev up` — Start the local development stack (docker-compose + moto services)
- `moto dev down` — Stop the local stack

The CLI defaults to connecting to `http://localhost:18080` for local dev, but can be configured to talk to remote moto-club instances via `--endpoint`.

**Key design decisions:**
- **Minimal configuration** — The CLI auto-detects the local stack and uses sensible defaults.
- **Clear feedback** — Commands show progress and results inline, so users always know what's happening.

## Keybox

**Secrets manager with SPIFFE-based identity and envelope encryption.**

Keybox is where garages and bikes fetch the secrets they need to operate. It's built on three core principles:

1. **Identity verification** — Every request must include a SPIFFE SVID (Ed25519-signed JWT) that identifies the workload. Keybox verifies the signature against the SPIRE trust bundle before authorizing access.
2. **Envelope encryption** — Each secret is encrypted with a unique AES-256 DEK (data encryption key), which is itself encrypted with a master KEK (key encryption key). Secrets are never stored in plaintext.
3. **Attribute-based access control (ABAC)** — Secrets are tagged with attributes (e.g., `garage_id=abc123`). Workloads can only fetch secrets whose attributes match the workload's identity claims.

Keybox also prevents enumeration attacks — callers can only fetch secrets they're authorized to see, and unauthorized requests return 404 (not 403) to avoid leaking secret existence.

**Key design decisions:**
- **No secret enumeration** — Garages can only access secrets explicitly granted to them, and cannot discover what other secrets exist.
- **Short-lived SVIDs** — Identity tokens expire after 15 minutes, limiting the blast radius of a compromised SVID.

## AI Proxy

**Credential-injecting reverse proxy for AI providers.**

The AI proxy solves a core problem: garages need to call AI providers (Anthropic, OpenAI, Google Gemini) but must never see the real API keys. The proxy sits in front of these providers, intercepts requests, fetches the real API key from keybox, and injects it into the upstream request.

From the garage's perspective, it sets its `ANTHROPIC_API_KEY` environment variable to its SVID JWT (which looks like `svid.xxxxx`). When the garage makes a request to the AI proxy, the proxy:

1. Extracts the SVID from the `Authorization` header
2. Verifies the SVID with keybox
3. Determines which real API key to use based on the request path (e.g., `/passthrough/anthropic/` → fetch Anthropic key)
4. Replaces the SVID with the real API key
5. Forwards the request to the provider

**Two modes:**
- **Passthrough** — `/passthrough/anthropic/`, `/passthrough/openai/`, etc. Forward provider-native requests with no translation. This is the primary path for Claude Code, which uses the Anthropic SDK directly.
- **Unified endpoint** — `/v1/chat/completions` accepts a common format and auto-routes by model name prefix (`claude-*`, `gpt-*`, `gemini-*`), translating request/response formats as needed.

**Key design decisions:**
- **Path allowlist** — Only inference endpoints are allowed. Admin APIs, billing, and fine-tuning are blocked.
- **Error sanitization** — Upstream errors are scrubbed to prevent API key leakage in error messages.
- **Rate limiting** — Uses moto-throttle to enforce per-garage rate limits, preventing abuse.

## Garage

**The development workspace where AI agents operate.**

Garages are Nix-built containers that provide a complete Rust development environment. They're ~3GB in size and include:

- Full Rust toolchain (cargo, rustc, clippy, rustfmt)
- Common CLI tools (git, jj, make, curl, etc.)
- Nix package manager for ad-hoc tool installation
- Terminal access via WireGuard tunnel and ttyd

Each garage runs in its own Kubernetes namespace with:

- **NetworkPolicy** — Deny-all ingress, scoped egress (can reach internet, DNS, keybox, ai-proxy, own supporting services)
- **ResourceQuota** — Limits on CPU, memory, and persistent storage
- **Security context** — Drop all capabilities, no privilege escalation, read-only root filesystem (except `/tmp` and `/workspace`)
- **No K8s API access** — `automountServiceAccountToken: false` prevents the garage from calling the Kubernetes API

Garages are ephemeral by default. They have a TTL (default 4 hours), and when the TTL expires, the garage and its namespace are deleted. Users can extend the TTL or close the garage early with `moto garage close`.

**Key design decisions:**
- **Container is the security perimeter** — The entire isolation model is built on the assumption that the container itself can be compromised. NetworkPolicies, ResourceQuotas, and no K8s API access ensure that a compromised garage can't escape its sandbox.
- **Nix for reproducibility** — Using Nix to build the image ensures that every garage has the same tooling and environment.

## Bike

**Minimal production container for running workloads built in garages.**

Bikes are the production counterpart to garages. They're designed to be:

- **Tiny** — <20MB, distroless base image
- **Immutable** — Read-only root filesystem, no shell, no package manager
- **Verifiable** — Built from the same Nix derivations as garages, so you can reproduce the build

Bikes follow an "engine contract" — they expect to find a binary at `/bin/engine` that implements a standard interface (HTTP server, health checks, graceful shutdown). The Moto tooling can generate Dockerfile and K8s manifests for bikes, and garages can build and test their engine before pushing to production.

**Key design decisions:**
- **No debugging tools in prod** — Bikes ship with no shell or utilities. If you need to debug, you do it in a garage, not in production.
- **Same tooling, different size** — Garages and bikes are built from the same Nix packages, so you can be confident that what works in the garage will work in the bike.

## WireGuard Tunnel

**Encrypted terminal access via userspace WireGuard and DERP relay.**

moto-wgtunnel provides secure remote access to garage terminals. It uses:

- **WireGuard** — Lightweight VPN protocol for encrypted connections
- **Userspace implementation** — No kernel modules or elevated privileges required
- **DERP relay** — For NAT traversal when direct UDP connections aren't possible

When a garage starts, moto-club generates a WireGuard keypair and registers the peer. The CLI gets the peer configuration and establishes a WireGuard tunnel to the garage. Traffic flows through the tunnel to ttyd (a web-based terminal server) running inside the garage.

**Key design decisions:**
- **Userspace only** — No kernel modules means simpler deployment and fewer security risks.
- **Short-lived tunnels** — WireGuard peers are tied to garage lifetimes. When the garage closes, the peer is deregistered.

## Throttle

**Rate limiting middleware for tower-based services.**

moto-throttle is a tower middleware layer that enforces rate limits on incoming requests. It supports:

- **Per-caller limits** — Extract caller identity from headers (e.g., SVID subject) and enforce per-caller rate limits
- **Redis-backed state** — Distributed rate limiting across multiple replicas using Redis as a shared state store
- **Sliding window algorithm** — More accurate than fixed windows, prevents burst abuse

Used by the AI proxy to limit per-garage API usage and prevent abuse.

**Key design decisions:**
- **Tower integration** — Works with any tower-based service (axum, tonic, hyper).
- **Fail-open on Redis unavailability** — If Redis is down, rate limiting is bypassed rather than blocking all traffic. This is a tradeoff: availability over strict enforcement.

## Cron

**TTL enforcement and scheduled cleanup tasks.**

Cron functionality lives inside the moto-club reconciliation loop rather than as a separate component. The reconciler runs every 30 seconds and:

- Checks for expired garages (TTL exceeded) and deletes them
- Cleans up orphaned namespaces (e.g., if a garage deletion fails partway through)
- Prunes old audit log entries beyond the retention window

**Key design decisions:**
- **Reconciler pattern** — Instead of cron jobs that run commands, the reconciler continuously drifts cluster state toward desired state. This is more robust (self-healing) and easier to test.
- **Leader election** — Only one replica runs the reconciler at a time, preventing duplicate deletions.

## Supporting Services

**Per-garage Postgres and Redis instances.**

When a garage requests supporting services, Moto deploys ephemeral Postgres and Redis instances into the garage's namespace. These are:

- **Ephemeral** — They're deleted when the garage closes
- **Isolated** — Each garage gets its own instances; garages never share databases
- **Minimal configuration** — Single-pod deployments with small resource requests, suitable for dev/test

In local dev (`moto dev up`), Postgres and Redis run via docker-compose and are shared across all garages to reduce resource usage.

**Key design decisions:**
- **Namespace isolation** — By running services in the garage's namespace, NetworkPolicies automatically prevent cross-garage access.
- **Dev vs prod tradeoff** — Local dev shares services for convenience; K8s deployments isolate them for security. The CLI and moto-club handle this difference transparently.

---

## The Motorcycle Metaphor

If you're wondering why things are named the way they are, here's the quick glossary:

- **Club** — Central meeting point where riders gather (moto-club orchestrates everything)
- **Garage** — A workspace where you build and maintain things (isolated dev environments)
- **Bike** — The finished product that goes out on the road (minimal production containers)
- **Keybox** — Where you keep your valuables locked up (secrets manager)
- **Throttle** — Controls how fast the engine runs (rate limiting)
- **Cron** — Engine maintenance schedule (scheduled tasks)

The metaphor is lightweight — it informs naming without being forced. You don't need to love motorcycles to use Moto.
