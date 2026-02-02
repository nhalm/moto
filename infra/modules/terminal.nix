# infra/modules/terminal.nix
# Terminal daemon (ttyd + tmux) for WebSocket terminal access
{ pkgs }:

let
  # Entrypoint script for the garage container
  # Starts ttyd listening on port 7681, spawning tmux sessions
  garageEntrypoint = pkgs.writeShellScriptBin "garage-entrypoint" ''
    set -e

    # Default working directory (can be overridden by WORKSPACE_DIR)
    WORKSPACE_DIR="''${WORKSPACE_DIR:-/workspace}"

    # Ensure workspace directory exists
    mkdir -p "$WORKSPACE_DIR"

    # Start ttyd with tmux
    # -p 7681: Listen on port 7681
    # -W: Writable (allow input)
    # tmux new-session -A -s garage: Create or attach to session named "garage"
    #   -A: Attach to existing session if it exists
    #   -s garage: Session name
    #   -c: Start in specified directory
    exec ${pkgs.ttyd}/bin/ttyd -p 7681 -W ${pkgs.tmux}/bin/tmux new-session -A -s garage -c "$WORKSPACE_DIR"
  '';
in
{
  contents = with pkgs; [
    ttyd
    tmux
    garageEntrypoint
  ];

  # The entrypoint script is exposed as 'garage-entrypoint' in PATH
  # Container Cmd should be set to [ "garage-entrypoint" ]
  #
  # No authentication - WireGuard tunnel is the auth boundary
}
