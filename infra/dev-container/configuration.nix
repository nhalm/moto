# infra/dev-container/configuration.nix
#
# NixOS system configuration for garage dev containers.
# This configures SSH, WireGuard support, and the development environment.
{ pkgs, devShellPackages }:

{ config, lib, modulesPath, ... }:

{
  imports = [
    # Container base configuration
    "${modulesPath}/profiles/minimal.nix"
  ];

  # System basics
  system.stateVersion = "24.11";

  # Boot configuration for containers
  boot.isContainer = true;

  # Networking
  networking = {
    hostName = "moto-garage";
    useDHCP = false;
    firewall = {
      enable = true;
      allowedTCPPorts = [ 22 ];
      allowedUDPPorts = [ 51820 ]; # WireGuard
    };
  };

  # SSH server for terminal access
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "yes";
      PasswordAuthentication = false;
    };
    # Host keys are generated on first boot
  };

  # Enable WireGuard kernel module
  # Note: WireGuard interface is configured dynamically by moto-garage-wgtunnel daemon
  boot.kernelModules = lib.mkIf (!config.boot.isContainer) [ "wireguard" ];

  # User configuration - root user for AI development
  users.users.root = {
    shell = pkgs.bashInteractive;
    # SSH keys are injected at runtime from moto-club
  };

  # Environment configuration
  environment = {
    # System packages from devShell
    systemPackages = devShellPackages ++ (with pkgs; [
      # Additional system utilities not in devShell
      cacert
      iana-etc
      nodejs_22  # Required for Claude Code
    ]);

    # Global environment variables
    variables = {
      HOME = "/root";
      TERM = "xterm-256color";
      SHELL = "/bin/bash";
      WORKSPACE = "/workspace";
      CARGO_HOME = "/root/.cargo";
      CARGO_TARGET_DIR = "/workspace/target";
      RUST_BACKTRACE = "1";
      RUST_LOG = "info";
      RUSTFLAGS = "-C linker=clang -C link-arg=-fuse-ld=mold";
      NIX_PATH = "nixpkgs=flake:nixpkgs";
      DO_NOT_TRACK = "1";
      PATH = "/root/.local/bin:$PATH";
    };

    # Shell configuration
    etc = {
      # CA certificates for HTTPS
      "ssl/certs/ca-bundle.crt".source = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";

      # Basic shell profile
      "profile.local".text = ''
        # Moto garage environment
        export PS1='\[\033[01;32m\]moto-garage\[\033[00m\]:\[\033[01;34m\]\w\[\033[00m\]\$ '

        # Change to workspace on login
        if [ -d /workspace ]; then
          cd /workspace
        fi
      '';
    };
  };

  # Time zone (UTC for consistency)
  time.timeZone = "UTC";

  # Security settings
  security = {
    # Allow passwordless sudo for root (AI needs full access)
    sudo.wheelNeedsPassword = false;
  };

  # Nix configuration
  nix = {
    settings = {
      experimental-features = [ "nix-command" "flakes" ];
      trusted-users = [ "root" ];
    };
  };

  # Programs configuration
  programs.bash = {
    enableCompletion = true;
    interactiveShellInit = ''
      # Source profile
      if [ -f /etc/profile.local ]; then
        source /etc/profile.local
      fi

      # Add Claude Code to PATH
      export PATH="$HOME/.local/bin:$PATH"
    '';
  };

  # Claude Code installation service
  # Installs Claude Code via the official shell script on first boot
  systemd.services.install-claude-code = {
    description = "Install Claude Code";
    wantedBy = [ "multi-user.target" ];
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      User = "root";
      ExecStart = pkgs.writeShellScript "install-claude-code" ''
        set -e

        # Check if already installed
        if [ -f /root/.local/bin/claude ]; then
          echo "Claude Code already installed"
          exit 0
        fi

        echo "Installing Claude Code..."
        mkdir -p /root/.local/bin

        # Install via official shell script
        ${pkgs.curl}/bin/curl -fsSL https://claude.ai/install.sh | ${pkgs.bash}/bin/bash

        # Verify installation
        if [ -f /root/.local/bin/claude ]; then
          echo "Claude Code installed successfully"
        else
          echo "Claude Code installation failed"
          exit 1
        fi
      '';
    };
  };
}
