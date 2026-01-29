# infra/pkgs/default.nix
# Exports all container package definitions
{ pkgs, rustToolchain }:

let
  motoBike = import ./moto-bike.nix { inherit pkgs; };
  motoClub = import ./moto-club.nix { inherit pkgs rustToolchain; };
in {
  moto-garage = import ./moto-garage.nix { inherit pkgs rustToolchain; };

  # Bike base image (minimal: CA certs, tzdata, non-root user)
  moto-bike = motoBike.image;

  # Engine binaries (for development/testing)
  moto-club-binary = motoClub.binary;

  # Final engine images (bike base + engine binary)
  # Built using mkBike helper per moto-bike.md spec
  moto-club-image = motoClub.image;

  # mkBike helper: creates final image from bike base + engine binary
  # Usage: mkBike { name = "club"; package = moto-club; }
  inherit (motoBike) mkBike;
}
