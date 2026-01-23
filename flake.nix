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

        rustToolchain = pkgs.rust-bin.stable."1.85.0".default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
        # Container image is only built for x86_64-linux
        garageImage = if system == "x86_64-linux" then
          pkgs.dockerTools.buildLayeredImage {
            name = "moto-dev";
            tag = "latest";

            contents = with pkgs; [
              # Base system
              bashInteractive
              coreutils
              gnugrep
              gnused
              gawk
              findutils
              which
              less
              procps
              util-linux
              cacert
              iana-etc
              nodejs_22  # Required for Claude Code

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

              # Connectivity
              wireguard-tools
              openssh
            ];

            config = {
              Cmd = [ "/bin/bash" ];
              WorkingDir = "/workspace";
              Env = [
                "HOME=/root"
                "TERM=xterm-256color"
                "SHELL=/bin/bash"
                "WORKSPACE=/workspace"
                "CARGO_HOME=/root/.cargo"
                "CARGO_TARGET_DIR=/workspace/target"
                "RUST_BACKTRACE=1"
                "RUST_LOG=info"
                "RUSTFLAGS=-C linker=clang -C link-arg=-fuse-ld=mold"
                "NIX_PATH=nixpkgs=flake:nixpkgs"
                "DO_NOT_TRACK=1"
                "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
                "PATH=/root/.local/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin"
              ];
              ExposedPorts = {
                "22/tcp" = {}; # SSH
              };
              Volumes = {
                "/workspace" = {};
                "/root/.cargo" = {};
                "/nix" = {};
              };
            };

            # Create necessary directories and files
            extraCommands = ''
              mkdir -p root
              mkdir -p root/.local/bin
              mkdir -p workspace
              mkdir -p etc/ssh
              mkdir -p etc/ssl/certs
              mkdir -p var/run
              mkdir -p tmp
              chmod 1777 tmp

              # Copy CA certificates
              cp ${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt etc/ssl/certs/ca-bundle.crt

              # Claude Code is installed at runtime via the install script
              # The script installs to ~/.local/bin/claude
            '';
          }
        else null;
      in {
        # Garage container image (only for x86_64-linux)
        packages = if garageImage != null then {
          garage = garageImage;
          default = garageImage;
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
