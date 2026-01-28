# infra/pkgs/default.nix
# Exports all container package definitions
{ pkgs, rustToolchain }:

{
  moto-garage = import ./moto-garage.nix { inherit pkgs rustToolchain; };
  moto-bike = import ./moto-bike.nix { inherit pkgs; };
}
