# Moto

> The Garage: secure workspaces for AI-assisted development

## What is Moto?

Moto provides secure, isolated development environments (garages) where AI agents like Claude Code can operate with full autonomy inside a container while being sandboxed from everything else. Each garage is a complete development workspace with its own tooling, secrets, and network isolation.

Built on Kubernetes, WireGuard, and SPIFFE-based identity, Moto ensures that AI agents have the freedom to explore, build, and experiment without compromising the security of your infrastructure. Garages are ephemeral, isolated by default, and designed to fail closed—a compromised garage can't affect other workspaces, the control plane, or the host.

Whether you're prototyping in a local k3d cluster or running production workloads, Moto's architecture treats the container as the security perimeter, using NetworkPolicies, encrypted tunnels, and envelope-encrypted secrets to keep AI agents productive but contained.

## How it works

```
┌──────────┐
│ moto CLI │  User runs: moto garage open
└─────┬────┘
      │
      v
┌─────────────┐        ┌──────────────────────────────────────┐
│ moto-club   │───────>│ Garage Pod (isolated namespace)      │
│ (orchestr.) │        │  - Dev container (Nix + Rust)        │
└─────────────┘        │  - WireGuard tunnel for terminal     │
      │                │  - SPIFFE SVID for identity          │
      │                │  - NetworkPolicy (deny-all ingress)  │
      v                └──────────────────────────────────────┘
┌─────────────┐                    │
│ Keybox      │<───────────────────┘
│ (secrets)   │  Garage fetches secrets using its SVID
└─────────────┘
      │
      v
┌─────────────┐
│ AI Proxy    │  Injects real API keys into AI provider requests
└─────────────┘
```

Flow: CLI → moto-club → K8s namespace + pod + NetworkPolicy + WireGuard peer + SVID. Garages authenticate to Keybox using short-lived Ed25519 JWTs, fetch secrets, and call AI providers through the proxy.

## Quickstart

See the **[Getting Started](https://github.com/nhalm/moto/wiki/getting-started)** guide for prerequisites and a walkthrough of your first garage.

Quick version:
```bash
# Start local dev stack
moto dev up

# Open a garage
moto garage open
```

## Documentation

| Page | Description |
|------|-------------|
| [Architecture](https://github.com/nhalm/moto/wiki/architecture) | Component map, design philosophy, data flow, motorcycle metaphor glossary |
| [Getting Started](https://github.com/nhalm/moto/wiki/getting-started) | Prerequisites, `moto dev up` walkthrough, first garage, stopping |
| [Deployment](https://github.com/nhalm/moto/wiki/deployment) | `make deploy`, what runs where, secrets, production considerations |
| [Security](https://github.com/nhalm/moto/wiki/security) | Threat model, isolation layers, SPIFFE SVIDs, keybox encryption, compliance |
| [AI Proxy](https://github.com/nhalm/moto/wiki/ai-proxy) | How AI credentials flow, passthrough vs unified, security, configuration |
| [Components](https://github.com/nhalm/moto/wiki/components) | Reference table and short sections for each component |

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
