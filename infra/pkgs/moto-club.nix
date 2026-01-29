# infra/pkgs/moto-club.nix
# Builds the moto-club engine binary and final image
# See specs/moto-bike.md for engine contract specification
{ pkgs, rustToolchain }:

let
  # Import the bike module for mkBike helper
  motoBike = import ./moto-bike.nix { inherit pkgs; };

  # Filter source to only include what's needed for the build
  src = pkgs.lib.cleanSourceWith {
    src = ../../.;
    filter = path: type:
      let
        baseName = baseNameOf path;
        relativePath = pkgs.lib.removePrefix (toString ../../. + "/") path;
      in
        # Include Cargo files
        baseName == "Cargo.toml" ||
        baseName == "Cargo.lock" ||
        # Include all crates
        pkgs.lib.hasPrefix "crates" relativePath ||
        # Exclude build artifacts and other unneeded files
        (type == "directory" && (
          baseName == "crates" ||
          baseName == "src"
        ));
  };

  # Build moto-club Rust binary from source
  # Uses musl target for static linking per moto-bike.md spec
  motoClubBinary = pkgs.rustPlatform.buildRustPackage {
    pname = "moto-club";
    version = "0.1.0";

    inherit src;

    # Build only the moto-club binary
    cargoBuildFlags = [ "--package" "moto-club" "--bin" "moto-club" ];

    # Disable tests for faster builds (tests run separately in CI)
    doCheck = false;

    # Required for building
    nativeBuildInputs = with pkgs; [
      pkg-config
    ];

    buildInputs = with pkgs; [
      # OpenSSL for TLS
      openssl
    ];

    # Set OpenSSL paths
    OPENSSL_NO_VENDOR = "1";

    # Cargo hash - must be updated when dependencies change
    # To update: run `nix build .#moto-club-image` and use the hash from the error
    cargoHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

    # Post-install: rename binary to match mkBike expectation
    # mkBike expects $out/bin/{name} where name="club"
    postInstall = ''
      if [ -f $out/bin/moto-club ]; then
        mv $out/bin/moto-club $out/bin/club
      fi
    '';
  };

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
