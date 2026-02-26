# infra/pkgs/moto-keybox.nix
# Builds the moto-keybox-server engine binary and final image
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

  # Build moto-keybox-server Rust binary from source
  motoKeyboxBinary = pkgs.rustPlatform.buildRustPackage {
    pname = "moto-keybox-server";
    version = "0.1.0";

    inherit src;

    # Build only the moto-keybox-server binary
    cargoBuildFlags = [ "--package" "moto-keybox-server" "--bin" "moto-keybox-server" ];

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
    # To update: run `nix build .#moto-keybox-image` and use the hash from the error
    cargoHash = "sha256-nGteJhBX3KpykL5Lun8x2fGxu6n8hUSMDuG50hoyjJ4=";

    # Post-install: rename binary to match mkBike expectation
    # mkBike expects $out/bin/{name} where name="keybox"
    postInstall = ''
      if [ -f $out/bin/moto-keybox-server ]; then
        mv $out/bin/moto-keybox-server $out/bin/keybox
      fi
    '';
  };

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
