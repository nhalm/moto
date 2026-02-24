# moto/flake.nix
{
  description = "Moto - fintech infrastructure";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable."1.85.0".minimal.override {
          extensions = [ "rust-src" "rust-analyzer" "rustfmt" "clippy" ];
        };
        # Import container packages from infra/pkgs/
        infraPkgs = import ./infra/pkgs { inherit pkgs rustToolchain; };
      in {
        # Container packages (Linux only - both x86_64 and aarch64)
        packages = if pkgs.stdenv.isLinux then {
          moto-garage = infraPkgs.moto-garage;
          moto-bike = infraPkgs.moto-bike;
          moto-club-image = infraPkgs.moto-club-image;
          moto-keybox-image = infraPkgs.moto-keybox-image;
          default = infraPkgs.moto-garage;
        } else {};

        # Export mkBike helper for building final engine images
        # Usage: nix eval .#lib.mkBike or import in other flakes
        lib = if pkgs.stdenv.isLinux then {
          inherit (infraPkgs) mkBike;
        } else {};

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust
            rustToolchain
            cargo-watch
            cargo-nextest
            cargo-audit
            cargo-deny
            cargo-edit
            cargo-expand
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
            redis

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
            k9s
            kubernetes-helm

            # AI
            # claude-code  # Not available in nixpkgs yet

            # Connectivity
            wireguard-tools
            openssh
          ];

          shellHook = ''
            export RUST_BACKTRACE=1
            export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold"
          '';
        };
      }
    );
}
