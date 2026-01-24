# infra/modules/ssh.nix
# SSH server configuration for container access
{ pkgs }:

{
  contents = with pkgs; [
    openssh
  ];

  # SSH configuration is handled at runtime via moto-garage-wgtunnel daemon
  # This module provides the openssh package for SSH server capabilities
}
