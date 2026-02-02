# infra/modules/terminal.nix
# Terminal daemon (ttyd + tmux) for WebSocket terminal access
{ pkgs }:

{
  contents = with pkgs; [
    ttyd
    tmux
  ];

  # ttyd is started at runtime as a daemon:
  # ttyd -p 7681 -W tmux new-session -A -s garage
  #
  # Flags:
  # -p 7681    Listen on port 7681
  # -W         Writable (allow input)
  #
  # tmux flags:
  # new-session -A -s garage  Create or attach to session named "garage"
  #
  # No authentication - WireGuard tunnel is the auth boundary
}
