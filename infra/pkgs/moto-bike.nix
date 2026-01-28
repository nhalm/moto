# infra/pkgs/moto-bike.nix
# Minimal base container image for production bikes
# See specs/moto-bike.md for full specification
{ pkgs }:

let
  # Bike base image - minimal runtime with CA certs and non-root user
  baseImage = pkgs.dockerTools.buildLayeredImage {
    name = "moto-bike";
    tag = "latest";

    # Minimal contents: only CA certificates and timezone data
    # Engines are statically compiled, so no libc needed
    contents = [
      pkgs.cacert   # TLS connections to external services
      pkgs.tzdata   # Correct timestamps in logs
    ];

    config = {
      User = "1000:1000";
      WorkingDir = "/";
      Env = [
        "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
        "TZDIR=${pkgs.tzdata}/share/zoneinfo"
      ];
    };

    # Create non-root user (1000:1000) for security
    fakeRootCommands = ''
      ${pkgs.dockerTools.shadowSetup}
      groupadd -g 1000 moto
      useradd -u 1000 -g moto -d / -s /sbin/nologin moto
    '';
    enableFakechroot = true;
  };

  # mkBike: Build final image from bike base + engine binary
  # Usage: mkBike { name = "club"; package = moto-club; }
  # Produces: moto-club image with /bin/club entrypoint
  mkBike = { name, package }: pkgs.dockerTools.buildLayeredImage {
    name = "moto-${name}";
    tag = "latest";
    fromImage = baseImage;
    contents = [ package ];
    config = {
      Entrypoint = [ "${package}/bin/${name}" ];
      User = "1000:1000";
      WorkingDir = "/";
      ExposedPorts = {
        "8080/tcp" = {};  # Main API (HTTP/gRPC)
        "8081/tcp" = {};  # Health endpoints
        "9090/tcp" = {};  # Prometheus metrics
      };
      Env = [
        "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
        "TZDIR=${pkgs.tzdata}/share/zoneinfo"
        "RUST_BACKTRACE=1"
      ];
    };
  };

in {
  # Export both the base image and the mkBike helper
  image = baseImage;
  inherit mkBike;
}
