# infra/pkgs/default.nix
# Exports all container package definitions
{ pkgs, rustToolchain }:

let
  motoBike = import ./moto-bike.nix { inherit pkgs; };
in {
  moto-garage = import ./moto-garage.nix { inherit pkgs rustToolchain; };

  # Bike base image (minimal: CA certs, tzdata, non-root user)
  moto-bike = motoBike.image;

  # mkBike helper: creates final image from bike base + engine binary
  # Usage: mkBike { name = "club"; package = moto-club; }
  inherit (motoBike) mkBike;
}
