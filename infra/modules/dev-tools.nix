# infra/modules/dev-tools.nix
# Development tooling for the garage container
{ pkgs, rustToolchain }:

{
  contents = with pkgs; [
    # Node.js (required for Claude Code)
    nodejs_22

    # Rust toolchain
    rustToolchain
    cargo-watch
    cargo-nextest
    mold
    sccache
    sqlx-cli

    # Build deps
    pkg-config
    openssl
    postgresql.lib
    clang

    # Version control
    git
    jujutsu
    gh

    # Database clients
    postgresql

    # General tools
    curl
    jq
    yq
    ripgrep
    fd
    bat
    htop
    tree

    # Kubernetes
    kubectl
  ];

  env = [
    "WORKSPACE=/workspace"
    "CARGO_HOME=/root/.cargo"
    "CARGO_TARGET_DIR=/workspace/target"
    "RUST_BACKTRACE=1"
    "RUST_LOG=info"
    "RUSTC_WRAPPER=sccache"
    "RUSTFLAGS=-C linker=clang -C link-arg=-fuse-ld=mold"
    "NIX_PATH=nixpkgs=flake:nixpkgs"
    # In dockerTools containers, binaries are symlinked to /bin
    "PATH=/root/.local/bin:/bin:/usr/bin"
  ];
}
