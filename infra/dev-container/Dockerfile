# infra/dev-container/Dockerfile
#
# Builds the moto-dev NixOS container image using Nix.
#
# IMPORTANT: This Dockerfile must be built from the repository root,
# as the container flake depends on the root flake for devShell packages.
#
# Usage (from repo root):
#   docker build -f infra/dev-container/Dockerfile -t moto-dev:latest .
#
# Alternative (recommended for local development):
#   cd infra/dev-container
#   nix build .#container
#   docker load < result
#
# The Nix approach is preferred because:
# - It produces a reproducible image from the flake.lock
# - It's faster when you have Nix installed (binary cache)
# - It doesn't require Docker-in-Docker or privileged builds

FROM nixos/nix:latest AS builder

# Enable flakes and configure Nix
RUN mkdir -p /etc/nix && \
    echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf && \
    echo "max-jobs = auto" >> /etc/nix/nix.conf

# Copy the entire repository
WORKDIR /build
COPY . .

# Build the container image using the flake
# This produces a Docker-loadable tarball
RUN nix build ./infra/dev-container#container \
    --no-link \
    --print-out-paths > /tmp/image-path.txt && \
    cp "$(cat /tmp/image-path.txt)" /tmp/image.tar.gz

# Stage 2: Create a minimal image that just contains the tarball
# The tarball should be extracted using `docker load`
FROM scratch AS output

COPY --from=builder /tmp/image.tar.gz /image.tar.gz

# To use this image:
# 1. Build: docker build -f infra/dev-container/Dockerfile -o type=tar,dest=nix-image.tar .
# 2. Extract: tar -xf nix-image.tar
# 3. Load: docker load < image.tar.gz
#
# Or in one step with BuildKit:
#   DOCKER_BUILDKIT=1 docker build -f infra/dev-container/Dockerfile --target=builder -o type=local,dest=out .
#   docker load < out/tmp/image.tar.gz
