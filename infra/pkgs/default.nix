# infra/pkgs/default.nix
# Exports all container package definitions
{ pkgs, rustToolchain }:

{
  moto-garage = import ./moto-garage.nix { inherit pkgs rustToolchain; };
  # moto-engine = import ./moto-engine.nix { inherit pkgs; };  # Future
}
