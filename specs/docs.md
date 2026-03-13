# Project Documentation

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Ready to Rip |
| Last Updated | 2026-03-13 |

## Changelog

### v0.1 (2026-03-13)
- Initial spec (draft)

## Overview

Public-facing documentation for the Moto project. A short README.md serves as the landing page, linking into a `docs/` folder that covers architecture, getting started, deployment, security, the AI proxy, and component details.

The `docs/` folder is the single source of truth. A GitHub Action publishes it to the repository's GitHub Wiki on push to main.

**Scope:**
- Project README.md (landing page)
- `docs/` folder with topical pages
- GitHub Action to publish `docs/` to the GitHub Wiki

**Out of scope:**
- API reference (auto-generated from code, future)
- Specs — these are internal and not referenced from docs

**Design principles:**
- Docs are self-contained — no links to `specs/`
- README is short — one screen, then links to docs
- Lean into the motorcycle metaphor

## Specification

### README.md

The project root README. Covers:

1. **Header** — Project name, tagline ("The Garage: secure workspaces for AI-assisted development"), badges if applicable
2. **What is Moto?** — 2-3 paragraphs. Moto provides secure, isolated development environments (garages) where AI agents like Claude Code can operate with full autonomy inside a container while being sandboxed from everything else. Built on Kubernetes, WireGuard, and SPIFFE-based identity.
3. **How it works** — Brief ASCII or text diagram showing the flow: CLI → moto-club → garage pod (with keybox, ai-proxy, WireGuard). Keep it high-level.
4. **Quickstart** — Point to `docs/getting-started.md`
5. **Documentation** — Table of links to each `docs/` page with one-line descriptions
6. **License** — Placeholder or actual license

Tone: Direct, slightly playful with the motorcycle metaphor. Not corporate.

### docs/architecture.md

High-level system design:

- **Component map** — ASCII diagram showing moto-club, keybox, ai-proxy, garage pods, WireGuard tunnels, Postgres, and how they connect
- **Design philosophy** — Why garages are isolated by default, why moto-club coordinates but never relays, why the container is the security perimeter
- **Data flow** — How a garage gets created: CLI → moto-club → K8s namespace + pod + NetworkPolicy + WireGuard peer + SVID
- **The motorcycle metaphor** — Quick glossary: Club (orchestrator), Garage (dev environment), Bike (production container), Keybox (secrets), etc.

### docs/getting-started.md

Prerequisites and first run:

- **Prerequisites** — Nix, Docker, k3d, Rust toolchain (whatever `moto dev up` needs)
- **Quick start** — `moto dev up` walkthrough: what it does (10 steps), what you see, how to verify it worked
- **Opening your first garage** — `moto garage open`, connecting via terminal, running code
- **Stopping** — Ctrl-C behavior, what persists (Postgres), what doesn't

### docs/deployment.md

Running Moto in a K8s cluster:

- **Local K8s deployment** — `make deploy`, what it sets up (moto-system namespace, StatefulSet Postgres, 3-replica Deployments)
- **What runs where** — moto-club, keybox, ai-proxy in `moto-system`; each garage in its own namespace
- **Secrets management** — How deploy-time secrets are generated and applied (`.dev/k8s-secrets/`, never in manifests)
- **Accessing the cluster** — `kubectl port-forward`, CLI default port 18080
- **Production considerations** — What would change for real production (HSM/KMS, TLS, external Postgres, RBAC)

### docs/security.md

The security model in depth:

- **Threat model** — AI agents run with full autonomy inside containers. The security goal is containment: a compromised garage cannot affect other garages, the control plane, or the host.
- **Isolation layers** — Container security context (drop ALL caps, no privilege escalation, seccomp), NetworkPolicy (deny-all ingress, scoped egress), ResourceQuota/LimitRange, no K8s API access (`automountServiceAccountToken: false`)
- **Identity (SPIFFE SVIDs)** — Ed25519-signed JWTs, 15-min TTL, bound to pod UID. How garages and bikes authenticate to keybox.
- **Secrets (Keybox)** — Envelope encryption (AES-256 DEK per secret, master KEK), ABAC access control, enumeration prevention
- **Network boundaries** — What garages can reach (internet, DNS, keybox, own supporting services) and what they cannot (moto-club, other garages, cloud metadata, WireGuard overlay)
- **Compliance** — SOC 2 alignment, fail-closed defaults, audit logging

### docs/ai-proxy.md

How AI credentials flow:

- **The problem** — Garages need to call AI providers (Anthropic, OpenAI, Gemini) but must never see real API keys
- **How it works** — ai-proxy sits in moto-system, fetches real keys from keybox, injects them into forwarded requests. Garages use their SVID JWT as a fake API key in env vars.
- **Passthrough mode** — `/passthrough/anthropic/` etc. forwards provider-native requests with no translation. Primary path for Claude Code.
- **Unified endpoint** — `/v1/chat/completions` auto-routes by model name prefix, translates formats
- **Security** — Path allowlist (inference only, no admin/billing), error sanitization (no key leakage), rate limiting via moto-throttle
- **Configuration** — How providers are registered, how keys are stored in keybox

### docs/components.md

Reference page for every major component:

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

Each component gets a short section: what it does, key design decisions, how it fits into the system.

### GitHub Action: Wiki Publish

A workflow at `.github/workflows/wiki-publish.yml`:

- **Trigger** — Push to `main` that changes `docs/**`
- **Action** — Checks out the wiki repo, syncs `docs/` contents, commits and pushes
- **Mapping** — Each `docs/foo.md` becomes a wiki page. `docs/README.md` or a `docs/Home.md` becomes the wiki sidebar/home if needed.

## Notes

- The motorcycle metaphor glossary in `docs/architecture.md` helps newcomers decode naming without it feeling forced
- Component table in `docs/components.md` serves as a quick-reference; detailed docs for security-critical components (keybox, ai-proxy, isolation) get their own pages
- Wiki publishing via CI means PRs can review doc changes before they go live
