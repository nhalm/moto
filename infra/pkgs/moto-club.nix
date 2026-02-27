# infra/pkgs/moto-club.nix
# Builds the moto-club engine binary and final image using crane
# See specs/moto-bike.md for engine contract specification
{ pkgs, craneLib, commonArgs, cargoArtifacts }:

let
  # Import the bike module for mkBike helper
  motoBike = import ./moto-bike.nix { inherit pkgs; };

  # Build moto-club Rust binary using crane
  # No cargoHash needed — crane vendors deps directly from Cargo.lock
  motoClubBinary = craneLib.buildPackage (commonArgs // {
    inherit cargoArtifacts;
    cargoExtraArgs = "--package moto-club --bin moto-club";

    # Disable tests (tests run separately in CI)
    doCheck = false;

    # Rename binary to match mkBike expectation
    # mkBike expects $out/bin/{name} where name="club"
    postInstall = ''
      if [ -f $out/bin/moto-club ]; then
        mv $out/bin/moto-club $out/bin/club
      fi
    '';
  });

  # Final moto-club image using mkBike helper
  # Creates: moto-club image with /bin/club entrypoint
  motoClubImage = motoBike.mkBike {
    name = "club";
    package = motoClubBinary;
  };

in {
  # Export both the binary and the final image
  binary = motoClubBinary;
  image = motoClubImage;
}
