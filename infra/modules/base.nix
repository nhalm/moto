# infra/modules/base.nix
# Common system settings for all containers
{ pkgs }:

{
  contents = with pkgs; [
    bashInteractive
    coreutils
    gnugrep
    gnused
    gawk
    findutils
    which
    less
    procps
    util-linux
    cacert
    iana-etc
  ];

  env = [
    "HOME=/root"
    "TERM=xterm-256color"
    "SHELL=/bin/bash"
    "DO_NOT_TRACK=1"
    "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
  ];
}
