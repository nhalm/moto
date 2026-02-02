# infra/pkgs/moto-garage.nix
# Container definition for the moto-garage (dev) container
{ pkgs, rustToolchain }:

let
  base = import ../modules/base.nix { inherit pkgs; };
  terminal = import ../modules/terminal.nix { inherit pkgs; };
  devTools = import ../modules/dev-tools.nix { inherit pkgs rustToolchain; };
  wireguard = import ../modules/wireguard.nix { inherit pkgs; };

  # Combine all contents from modules
  allContents = base.contents ++ terminal.contents ++ devTools.contents ++ wireguard.contents;

  # Combine all environment variables from modules
  allEnv = base.env ++ devTools.env;

  # Use buildEnv to handle file collisions between packages
  # This is required because multiple packages have 'share' directories
  garageEnv = pkgs.buildEnv {
    name = "garage-env";
    paths = allContents;
    # pathsToLink defaults to all, which is what we want
  };
in
pkgs.dockerTools.buildLayeredImage {
  name = "moto-garage";
  tag = "latest";

  # Use the buildEnv instead of raw contents to avoid collisions
  contents = [ garageEnv ];

  config = {
    Cmd = [ "/bin/bash" ];
    WorkingDir = "/workspace";
    Env = allEnv;
    ExposedPorts = {
      "7681/tcp" = {}; # ttyd (WebSocket terminal)
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
    mkdir -p var/run
    mkdir -p tmp
    chmod 1777 tmp

    # CA certificates are provided by cacert in the buildEnv
    # and SSL_CERT_FILE env var points to the correct location

    # Claude Code is installed at runtime via the install script
    # The script installs to ~/.local/bin/claude
  '';
}
