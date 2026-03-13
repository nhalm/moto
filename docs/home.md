# Moto Documentation

> **The Garage**: secure workspaces for AI-assisted development.

Moto provides isolated development environments — **garages** — where AI agents like Claude Code operate with full autonomy inside a container while being sandboxed from everything else. Built on Kubernetes, WireGuard, and SPIFFE-based identity.

## How It Works

An AI agent in a garage has root access, installs packages, writes code, runs tests — whatever it needs. But it can't escape the container, reach other garages, or access the control plane. Real API keys never touch the garage; the **AI proxy** injects credentials on the fly so the agent never sees them.

```
moto CLI ──► moto-club ──► K8s namespace (per garage)
                │               ├── dev container (Claude Code)
                │               ├── postgres (optional)
                │               └── redis (optional)
                │
                ├── keybox (secrets + SPIFFE identity)
                ├── ai-proxy (credential injection)
                └── WireGuard tunnel (terminal access)
```

## Documentation

### Getting Started
- **[Getting Started](getting-started)** — Prerequisites, first garage, local dev

### Architecture & Design
- **[Architecture](architecture)** — Component map, data flow, design philosophy
- **[Components](components)** — Reference for every major component

### Operations
- **[Deployment](deployment)** — K8s deployment, what runs where, production considerations

### Security
- **[Security](security)** — Threat model, isolation, SPIFFE identity, encryption
- **[AI Proxy](ai-proxy)** — How AI credentials flow without exposure

## Quick Start

```bash
# Start the local dev stack
moto dev up

# Open a garage
moto garage open --repo https://github.com/your-org/your-repo

# Connect via terminal
# (WireGuard tunnel connects automatically)
```

See the [Getting Started guide](getting-started) for the full walkthrough.

## Key Concepts

| Term | What It Is |
|------|-----------|
| **Garage** | Isolated dev environment — a K8s namespace with a dev container |
| **Bike** | Minimal production container (<20MB), runs the engine contract |
| **Club** (moto-club) | Central orchestrator — manages garages, WireGuard peers, reconciliation |
| **Keybox** | Secrets manager with SPIFFE identity and envelope encryption |
| **AI Proxy** | Reverse proxy that injects real API keys so garages never see them |
| **Engine** | The health/readiness contract every service implements |

## Links

- **[GitHub Repository](https://github.com/nhalm/moto)**
- **[Issues & Support](https://github.com/nhalm/moto/issues)**
