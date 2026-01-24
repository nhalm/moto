# infra/modules/wireguard.nix
# WireGuard support for tunnel connectivity
{ pkgs }:

{
  contents = with pkgs; [
    wireguard-tools
  ];

  # WireGuard configuration is handled dynamically by moto-garage-wgtunnel daemon
  # which registers with moto-club on startup and configures the interface
  # See: moto-wgtunnel.md for details
}
