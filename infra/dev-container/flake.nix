# infra/dev-container/flake.nix
#
# Container-specific flake that imports the root flake and builds
# the NixOS container image for garage environments.
{
  description = "Moto dev container - NixOS garage environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Import root flake for devShell packages
    root.url = "path:../..";
    root.inputs.nixpkgs.follows = "nixpkgs";
    root.inputs.rust-overlay.follows = "rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, root }:
    let
      # Container image is only built for x86_64-linux
      containerSystem = "x86_64-linux";

      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs {
        system = containerSystem;
        inherit overlays;
      };

      # Get the devShell packages from root flake
      rootDevShell = root.devShells.${containerSystem}.default;

      # Import the NixOS configuration
      nixosConfig = import ./configuration.nix {
        inherit pkgs;
        devShellPackages = rootDevShell.buildInputs;
      };
    in
    {
      # NixOS configuration for the container
      nixosConfigurations.dev-container = nixpkgs.lib.nixosSystem {
        system = containerSystem;
        modules = [
          nixosConfig
        ];
      };

      # Container image output
      packages.${containerSystem} = {
        container = pkgs.dockerTools.buildLayeredImage {
          name = "moto-garage";
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

            # From devShell
          ] ++ rootDevShell.buildInputs;

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
            mkdir -p var/run
            mkdir -p tmp
            chmod 1777 tmp

            # Claude Code is installed at runtime via the install script
            # See configuration.nix for the systemd service that handles this
            # The script installs to ~/.local/bin/claude
          '';
        };

        # Default package is the container
        default = self.packages.${containerSystem}.container;
      };

      # Also provide devShell on the container system
      devShells.${containerSystem}.default = rootDevShell;
    }

    # Add cross-platform devShell support for local development
    // flake-utils.lib.eachSystem [ "x86_64-darwin" "aarch64-darwin" "aarch64-linux" ] (system:
      let
        pkgsLocal = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
      in {
        devShells.${system}.default = root.devShells.${system}.default;
      }
    );
}
