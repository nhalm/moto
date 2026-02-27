# infra/pkgs/moto-keybox.nix
# Builds the moto-keybox-server engine binary and final image using crane
# See specs/moto-bike.md for engine contract specification
{ pkgs, craneLib, commonArgs, cargoArtifacts }:

let
  # Import the bike module for mkBike helper
  motoBike = import ./moto-bike.nix { inherit pkgs; };

  # Build moto-keybox-server Rust binary using crane
  # No cargoHash needed — crane vendors deps directly from Cargo.lock
  motoKeyboxBinary = craneLib.buildPackage (commonArgs // {
    inherit cargoArtifacts;
    cargoExtraArgs = "--package moto-keybox-server --bin moto-keybox-server";

    # Disable tests (tests run separately in CI)
    doCheck = false;

    # Rename binary to match mkBike expectation
    # mkBike expects $out/bin/{name} where name="keybox"
    postInstall = ''
      if [ -f $out/bin/moto-keybox-server ]; then
        mv $out/bin/moto-keybox-server $out/bin/keybox
      fi
    '';
  });

  # Final moto-keybox image using mkBike helper
  # Creates: moto-keybox image with /bin/keybox entrypoint
  motoKeyboxImage = motoBike.mkBike {
    name = "keybox";
    package = motoKeyboxBinary;
  };

in {
  # Export both the binary and the final image
  binary = motoKeyboxBinary;
  image = motoKeyboxImage;
}
