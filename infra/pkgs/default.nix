# infra/pkgs/default.nix
# Exports all container package definitions
{ pkgs, rustToolchain, craneLib, commonArgs, cargoArtifacts }:

let
  motoBike = import ./moto-bike.nix { inherit pkgs; };
  motoClub = import ./moto-club.nix { inherit pkgs craneLib commonArgs cargoArtifacts; };
  motoKeybox = import ./moto-keybox.nix { inherit pkgs craneLib commonArgs cargoArtifacts; };
in {
  moto-garage = import ./moto-garage.nix { inherit pkgs rustToolchain; };

  # Bike base image (minimal: CA certs, tzdata, non-root user)
  moto-bike = motoBike.image;

  # Engine binaries (for development/testing)
  moto-club-binary = motoClub.binary;
  moto-keybox-binary = motoKeybox.binary;

  # Final engine images (bike base + engine binary)
  # Built using mkBike helper per moto-bike.md spec
  moto-club-image = motoClub.image;
  moto-keybox-image = motoKeybox.image;

  # mkBike helper: creates final image from bike base + engine binary
  # Usage: mkBike { name = "club"; package = moto-club; }
  inherit (motoBike) mkBike;
}
