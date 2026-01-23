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
    '';
  };
}
