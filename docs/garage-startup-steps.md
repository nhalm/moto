# Getting a Garage Running: Step-by-Step Reality

What it actually took to go from zero to a running garage pod, including every issue hit along the way.

## Prerequisites

- Docker / Colima running (with enough disk — at least 20GB free in the VM)
- k3d installed
- Nix installed (for dev shell)

## Step 1: Enter dev shell

```bash
nix develop
```

This gives you the Rust toolchain, cargo, sqlx-cli, kubectl, k3d, etc.

## Step 2: Create k3d cluster

```bash
make dev-cluster
# runs: cargo run --bin moto -- cluster init
```

This creates a k3d cluster named `moto` with a registry at `localhost:5050` (accessible inside k3d as `moto-registry:5000`).

**Issue encountered:** The `moto cluster init` binary name was wrong in the Makefile (was `moto-cli`). Fixed in commit `32b6802`.

## Step 3: Build the garage container image

```bash
make build-garage
```

This runs `nix build` inside a Docker container (nixos/nix) to produce a Linux container image, then pipes it into `docker load`.

**Issues encountered:**
- Image was 4.8GB compressed (9.8GB uncompressed). Caused disk pressure in k3d later. Fixed by switching to `.minimal` Rust profile and dropping clang (~2.5GB compressed after fix).
- Build takes ~15-20 minutes the first time (Nix downloading everything). Subsequent builds are faster due to the `nix-store` Docker volume cache.
- The `nix-store` Docker volume itself uses ~8.7GB.

## Step 4: Push the garage image to the k3d registry

```bash
make push-garage
# REGISTRY defaults to localhost:5050
```

**Issues encountered:**
- Originally the registry port was wrong (`localhost:5000` vs `localhost:5050`). Fixed to match k3d config.
- The image was being stored in 3-4 places simultaneously: Docker daemon (~10GB), registry (~4.4GB), k3d containerd (~4.8GB), nix-store cache (~8.7GB). Total ~28GB for one image.
- After fix: `push-garage` now cleans up the Docker daemon copy after pushing (saves ~10GB VM disk).
- Old image tags linger in the registry. Had to manually delete old tags and run garbage collection to free space:
  ```bash
  # Delete old tag directory inside registry container
  docker exec k3d-moto-registry rm -rf /var/lib/registry/docker/registry/v2/repositories/moto-garage/_manifests/tags/<old-sha>
  # Run GC
  docker exec k3d-moto-registry bin/registry garbage-collect /etc/docker/registry/config.yml --delete-untagged
  ```

## Step 5: Start the development database

```bash
make dev-db-up
# runs: docker compose up -d --wait
```

Starts a Postgres 16 instance on port 5432 with two databases: `moto_club` and `moto_keybox`.

## Step 6: Generate keybox keys

```bash
make dev-keybox-init
```

Creates `.dev/keybox/master.key`, `.dev/keybox/signing.key`, and `.dev/keybox/service-token`. Idempotent — skips if files already exist.

## Step 7: Run database migrations

```bash
make dev-db-migrate
# runs: cargo sqlx migrate run --source crates/moto-club-db/migrations
```

## Step 8: Start moto-keybox

```bash
make dev-keybox
# runs moto-keybox-server with env vars pointing to .dev/keybox/ keys
```

Runs in foreground. Keybox auto-runs its own migrations on startup.

**Issues encountered:**
- Keybox and club both default to port 8080. Dev config puts keybox on 8090 (API) and 8091 (health).
- moto-club needed `MOTO_CLUB_KEYBOX_HEALTH_URL` pointing to port 8091 (not the API port). Missing initially, fixed in commit `73c1d79`.

## Step 9: Start moto-club

```bash
make dev-club
# runs moto-club with env vars pointing to local keybox and k3d cluster
```

Runs in foreground in a separate terminal.

**Issues encountered:**
- Missing `MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE` env var. Club couldn't authenticate to keybox for SVID issuance. Fixed in commit `bc8a14e`.
- `MOTO_CLUB_DEV_CONTAINER_IMAGE` must use `moto-registry:5000/moto-garage:latest` (the in-cluster registry name), not `localhost:5050` (host-only).

## Step 10: Open a garage

```bash
MOTO_USER=nick cargo run --bin moto -- garage open --no-attach
```

**Issues encountered (in order):**

1. **`garage-entrypoint: executable file not found in $PATH`**: The pod spec mounted an EmptyDir at `/nix`, which shadowed the image's `/nix/store` contents. All symlinks from `/bin` into `/nix/store/` broke. Fixed by removing the `/nix` volume and mount from pods.rs (commit `5ce7c5d`, `8713bab`).

2. **Disk pressure eviction**: kubelet evicted the pod because the k3d node had less than ~2.5GB free. The 4.8GB image consumed too much space. Fixed by slimming the image to 2.5GB (commit `0a5de28`).

3. **Old images consuming registry space**: After rebuilding a smaller image, the old 4.8GB image was still in the registry. Had to manually clean the registry (see Step 4).

4. **ContainerStatusUnknown**: Machine sleeping caused kubelet to lose track of the container. Had to close the garage and open a fresh one.

## Step 11: Verify the garage is working

```bash
kubectl get pods -n moto-garage-<id>
# Should show Running, 1/1 Ready

# Check ttyd is serving
kubectl exec -n moto-garage-<id> <pod-name> -- curl -s http://localhost:7681
# Should return HTML

# Check tools are available
kubectl exec -n moto-garage-<id> <pod-name> -- rustc --version
kubectl exec -n moto-garage-<id> <pod-name> -- cargo --version
kubectl exec -n moto-garage-<id> <pod-name> -- git --version
```

## Shortcut: `make dev-up`

`make dev-up` combines steps 5-9 (db, keybox-init, migrate, start keybox in background, start club in foreground). But you still need to do steps 1-4 and 10 manually.

---

## Summary of Bugs Fixed During This Process

| Bug | Root Cause | Fix |
|-----|-----------|-----|
| `garage-entrypoint: not found` | EmptyDir at `/nix` shadowed image's `/nix/store` | Remove `/nix` volume and mount |
| Disk pressure eviction | 4.8GB image too large for k3d VM | Slim to 2.5GB (drop clang, rust-docs) |
| Image stored 4x (~28GB) | Docker daemon + registry + containerd + nix cache | Clean up Docker daemon copy after push |
| Missing keybox health URL | Club checked wrong port for keybox health | Add `MOTO_CLUB_KEYBOX_HEALTH_URL` |
| Missing service token env | Club couldn't auth to keybox | Add `MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE` |
| Wrong registry port | `localhost:5000` vs `localhost:5050` | Fix all references to `localhost:5050` |
| Wrong container image ref | `localhost:5050` vs `moto-registry:5000` | Use in-cluster name for pod image |
| Wrong binary name in Makefile | `moto-cli` vs `moto` | Fix `dev-cluster` target |

## What Would Make This Simpler

### The core friction points:

1. **10 steps to get one garage running.** `make dev-up` helps but still requires cluster creation, image build/push, and manual garage open separately.

2. **Image build is slow and huge.** 15-20 min first build, 2.5GB compressed image. Nix-in-Docker adds complexity. The nix-store cache volume is 8.7GB.

3. **Disk space management is manual.** Old images accumulate in the registry. No automatic garbage collection. k3d VM disk fills up silently until pods get evicted.

4. **Too many env vars to wire correctly.** 6 env vars for keybox, 6 for club. Ports, URLs, file paths all need to match. Easy to get wrong.

5. **Three terminals needed.** Keybox, club, and then a shell for garage commands. `dev-up` combines two but it's fragile (background process management in Make).

6. **No feedback when things go wrong.** Pod eviction due to disk pressure shows as a generic error. Image pull failures don't say why. Entrypoint failures require kubectl describe to diagnose.

7. **Registry cleanup is manual.** No `make clean-registry` target. Have to exec into the registry container and run commands by hand.

8. **First-time setup has too many one-time steps** that aren't obvious: cluster creation, image build, key generation. Each can fail independently with different error modes.
