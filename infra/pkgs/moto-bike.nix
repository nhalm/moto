# infra/pkgs/moto-bike.nix
# Minimal base container image for production bikes
# See specs/moto-bike.md for full specification
{ pkgs }:

pkgs.dockerTools.buildLayeredImage {
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
}
