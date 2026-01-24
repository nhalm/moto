# infra/pkgs/moto-garage.nix
# Container definition for the moto-garage (dev) container
{ pkgs, rustToolchain }:

let
  base = import ../modules/base.nix { inherit pkgs; };
  ssh = import ../modules/ssh.nix { inherit pkgs; };
  devTools = import ../modules/dev-tools.nix { inherit pkgs rustToolchain; };
  wireguard = import ../modules/wireguard.nix { inherit pkgs; };

  # Combine all contents from modules
  allContents = base.contents ++ ssh.contents ++ devTools.contents ++ wireguard.contents;

  # Combine all environment variables from modules
  allEnv = base.env ++ devTools.env;
in
pkgs.dockerTools.buildLayeredImage {
  name = "moto-garage";
  tag = "latest";

  contents = allContents;

  config = {
    Cmd = [ "/bin/bash" ];
    WorkingDir = "/workspace";
    Env = allEnv;
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
