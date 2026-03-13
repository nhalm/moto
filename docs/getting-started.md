# Getting Started

This guide walks you through setting up Moto locally and opening your first garage.

## Prerequisites

Before running Moto, you'll need:

- **Nix** (with flakes enabled) — Used to build the dev container image
- **Docker** — For running k3d and local container registry
- **k3d** — Lightweight Kubernetes (k3s) in Docker
- **Rust toolchain** — 1.75 or later (if building from source)
- **kubectl** — Kubernetes command-line tool (optional, for debugging)

### Installing Prerequisites

**macOS (Homebrew):**
```bash
# Install Nix with flakes enabled
curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install

# Install Docker Desktop (or use Colima)
brew install --cask docker

# Install k3d
brew install k3d

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# (Optional) Install kubectl
brew install kubectl
```

**Linux (Debian/Ubuntu):**
```bash
# Install Nix with flakes enabled
curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install

# Install Docker
sudo apt-get update
sudo apt-get install docker.io
sudo usermod -aG docker $USER  # Log out and back in after this

# Install k3d
curl -s https://raw.githubusercontent.com/k3d-io/k3d/main/install.sh | bash

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# (Optional) Install kubectl
sudo snap install kubectl --classic
```

## Quick Start

The `moto dev up` command starts the full local development stack:

```bash
# Clone the repository
git clone https://github.com/your-org/moto.git
cd moto

# Start local dev stack
moto dev up
```

### What `moto dev up` Does

Behind the scenes, this command performs **10 steps**:

1. **Creates k3d cluster** — Spins up a local Kubernetes cluster named `moto-dev` using k3d
2. **Sets up local registry** — Creates a Docker registry at `localhost:5050` for container images
3. **Starts Postgres (Docker Compose)** — Runs Postgres 15 for moto-club, keybox, and audit logs
4. **Generates secrets** — Creates master encryption keys, service tokens, and database credentials (stored in `.dev/k8s-secrets/`)
5. **Builds dev container** — Uses Nix to build the garage container image (~3GB, includes Rust toolchain and dev tools)
6. **Pushes image to registry** — Tags and pushes the dev container to `localhost:5050/moto-garage:latest`
7. **Starts moto-club** — Runs the orchestrator service via `cargo run` (watches for code changes)
8. **Starts keybox** — Runs the secrets manager via `cargo run`
9. **Starts ai-proxy** — Runs the credential-injecting reverse proxy via `cargo run`
10. **Waits for readiness** — Polls health endpoints until all services are ready

**What you'll see:**
```
[moto] Creating k3d cluster...
[moto] Starting local registry at localhost:5050...
[moto] Starting Postgres (docker-compose)...
[moto] Generating secrets...
[moto] Building dev container (this may take a few minutes)...
[moto] Pushing moto-garage:latest to registry...
[moto] Starting moto-club on :18080...
[moto] Starting keybox on :19090...
[moto] Starting ai-proxy on :17070...
[moto] All services ready!

moto-club:  http://localhost:18080
keybox:     http://localhost:19090
ai-proxy:   http://localhost:17070
```

### Verifying the Stack

Check that all services are healthy:

```bash
# Check moto-club
curl http://localhost:18080/health

# Check keybox
curl http://localhost:19090/health

# Check ai-proxy
curl http://localhost:17070/health

# List Kubernetes nodes (should show 1 server node)
kubectl get nodes

# List namespaces (should include moto-system)
kubectl get namespaces
```

All health endpoints should return `{"status": "ok"}`.

## Opening Your First Garage

Once the dev stack is running, you can open a garage:

```bash
moto garage open
```

This command:

1. Sends a request to moto-club to create a new garage
2. Allocates a WireGuard IP and generates a garage ID (e.g., `garage-abc123`)
3. Creates a Kubernetes namespace and pod for the garage
4. Establishes a WireGuard tunnel between your machine and the garage
5. Connects you to an interactive terminal inside the garage container

**What you'll see:**
```
[moto] Creating garage...
[moto] Garage ID: garage-abc123
[moto] WireGuard tunnel established (10.42.1.5)
[moto] Connecting to terminal...

Welcome to your garage!

You are now inside an isolated development environment.
Run `exit` to close the terminal (garage will remain running).

root@garage-abc123:/#
```

### Inside the Garage

The garage container includes:

- **Full Rust toolchain** — `cargo`, `rustc`, `clippy`, `rustfmt`
- **Nix package manager** — Install additional tools with `nix-env -iA nixpkgs.<package>`
- **Git** — Clone repositories, commit changes
- **Build tools** — `make`, `cmake`, `gcc`, `clang`
- **Shell** — `bash`, `zsh`, `fish` (default: bash)

Try running some commands:

```bash
# Check Rust version
rustc --version

# Check available memory
free -h

# Fetch a secret from keybox (requires MOTO_GARAGE_SVID env var)
curl -H "Authorization: Bearer $MOTO_GARAGE_SVID" \
     http://keybox.moto-system.svc.cluster.local:9090/secrets/example

# Call an AI provider through the proxy
export ANTHROPIC_API_KEY="garage-abc123"
curl -X POST http://ai-proxy.moto-system.svc.cluster.local:8080/passthrough/anthropic/v1/messages \
     -H "Authorization: Bearer $ANTHROPIC_API_KEY" \
     -H "Content-Type: application/json" \
     -d '{"model": "claude-3-5-sonnet-20241022", "messages": [{"role": "user", "content": "Hello!"}], "max_tokens": 100}'
```

### Running Code in the Garage

Clone a repository and build it:

```bash
# Clone a Rust project
git clone https://github.com/example/my-rust-project.git
cd my-rust-project

# Build it
cargo build

# Run tests
cargo test
```

The garage has internet access, so you can fetch dependencies from crates.io, npmjs.com, or any other package registry.

## Managing Garages

### List All Garages

```bash
moto garage list
```

Output:
```
ID              OWNER                   STATUS    CREATED             TTL
garage-abc123   user@example.com        running   2026-03-13 10:30    3h 45m
garage-def456   user@example.com        running   2026-03-13 09:15    2h 30m
```

### Reconnect to an Existing Garage

If you exit the terminal, the garage keeps running. Reconnect with:

```bash
moto garage attach garage-abc123
```

### Close a Garage

When you're done, close the garage to free up resources:

```bash
moto garage close garage-abc123
```

This deletes the Kubernetes namespace, pod, and all ephemeral data (Postgres, Redis, workspace files). Garages are designed to be ephemeral—commit your work to Git before closing.

## Stopping the Dev Stack

Press **Ctrl-C** in the terminal where you ran `moto dev up`. This:

- Stops moto-club, keybox, and ai-proxy (graceful shutdown)
- Closes all running garages
- Stops the local registry
- **Keeps k3d cluster and Postgres running** (for faster restarts)

### Full Cleanup

To delete everything (cluster, volumes, database):

```bash
moto dev down --clean
```

This removes:
- k3d cluster (`k3d cluster delete moto-dev`)
- Local registry
- Postgres data volume
- Generated secrets in `.dev/k8s-secrets/`

**Warning:** This destroys all garages and their data. Only use `--clean` when you're sure you don't need anything from the cluster.

## Next Steps

Now that you have a working garage, you can:

- **Read [architecture.md](architecture.md)** — Understand how components interact and the security model
- **Read [security.md](security.md)** — Learn about isolation layers, SPIFFE identity, and compliance
- **Read [deployment.md](deployment.md)** — Deploy Moto to a real Kubernetes cluster
- **Read [ai-proxy.md](ai-proxy.md)** — Configure AI provider credentials and endpoints
- **Read [components.md](components.md)** — Reference documentation for all services

## Troubleshooting

### Port Conflicts

If port 18080, 19090, or 17070 is already in use, edit the port mappings in `docker-compose.yml` or pass custom ports via environment variables:

```bash
CLUB_PORT=28080 KEYBOX_PORT=29090 AI_PROXY_PORT=27070 moto dev up
```

### Container Build Fails

If the Nix build times out or fails, ensure you have enough disk space (at least 10GB free) and a stable internet connection. Retry with:

```bash
moto dev build --force
```

### WireGuard Tunnel Fails

If the tunnel fails to establish:

1. Check that UDP port 51820 is not blocked by your firewall
2. Verify the garage pod is running: `kubectl get pods -n moto-garage-abc123`
3. Check moto-club logs: `moto logs club`

### Service Not Ready

If a service fails the health check:

```bash
# Check moto-club logs
moto logs club

# Check keybox logs
moto logs keybox

# Check ai-proxy logs
moto logs ai-proxy

# Check Postgres connectivity
docker-compose -f docker-compose.yml ps
```

## Development Workflow

For active development on Moto itself:

1. **Make code changes** — Edit files in `crates/`
2. **Restart services** — Services auto-reload on code changes (via `cargo watch`)
3. **Test changes** — Open a garage and verify behavior
4. **Run tests** — `make test` (runs unit and integration tests)
5. **Lint** — `make lint` (runs clippy and rustfmt)

See the [Makefile](../Makefile) for all available targets (`make help`).
