{
  pkgs,
  craneLib,
  tools,
  postgres,
}:
let
  inherit (pkgs) lib;
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../.cargo
      ../.sqlx
    ];
  };
  common = {
    inherit src;
    pname = "ferret";
    version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;
    strictDeps = true;
    SQLX_OFFLINE = "true";
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.cmake
    ];
    buildInputs = [ pkgs.openssl ];
  };
  cargoArtifacts = craneLib.buildDepsOnly (
    common
    // {
      cargoExtraArgs = "--workspace --locked";
    }
  );
  # Service-fixture changes must not discard compiled integration-test artifacts.
  testArtifacts = craneLib.cargoTest (
    common
    // {
      inherit cargoArtifacts;
      cargoExtraArgs = "--workspace --locked --no-run";
    }
  );
  package =
    name:
    craneLib.buildPackage (
      common
      // {
        inherit cargoArtifacts;
        cargoExtraArgs = "--locked -p ${name}";
        doCheck = false;
      }
    );
  server = package "ferret-server";
  plugins = package "ferret-plugins";
  services = {
    nativeBuildInputs = common.nativeBuildInputs ++ [
      postgres
      pkgs.dovecot
      pkgs.git
      tools.sqlx
    ];
    SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    SSL_CERT_DIR = "${pkgs.cacert}/etc/ssl/certs";
    preBuild = ''
      source ${./check-services.sh}
    '';
  };
in
{
  inherit
    server
    plugins
    postgres
    cargoArtifacts
    ;
  checks = {
    rust-fmt = craneLib.cargoFmt common;
    rust-clippy = craneLib.cargoClippy (
      common
      // {
        inherit cargoArtifacts;
        cargoExtraArgs = "--workspace --all-targets --locked";
        cargoClippyExtraArgs = "-- -D warnings";
      }
    );
    rust-test = craneLib.cargoTest (
      common
      // services
      // {
        cargoArtifacts = testArtifacts;
        doInstallCargoArtifacts = false;
        cargoExtraArgs = "--workspace --locked";
        RUST_TEST_THREADS = "6";
      }
    );
    sqlx = craneLib.mkCargoDerivation (
      common
      // services
      // {
        inherit cargoArtifacts;
        buildPhaseCargoCommand = ''
          cargo sqlx migrate run --source crates/ferret-server/migrations
          SQLX_OFFLINE=false cargo sqlx prepare --workspace --check -- --locked
        '';
        doCheck = false;
        doInstallCargoArtifacts = false;
        installPhaseCommand = "touch $out";
      }
    );
    graphql-schema = pkgs.runCommand "ferret-graphql-schema" { } ''
      ${server}/bin/ferret schema > schema.graphql
      diff -u ${../frontend/schema.graphql} schema.graphql
      touch "$out"
    '';
  };
}
