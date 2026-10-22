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
  };
  # Only the shipped packages build on these, so they skip the tests' dev-dependencies.
  cargoArtifacts = craneLib.buildDepsOnly (
    common
    // {
      cargoExtraArgs = "--workspace --locked";
      doCheck = false;
    }
  );
  # The checks ship nothing, so they skip release optimization and debug info, which dominate
  # compiling and linking the test binaries. Their deps need artifacts of their own: cargo
  # fingerprints differ per profile and env.
  dev = common // {
    CARGO_PROFILE = "";
    CARGO_PROFILE_DEV_DEBUG = "0";
  };
  devArtifacts = craneLib.buildDepsOnly (
    dev
    // {
      cargoExtraArgs = "--workspace --locked";
    }
  );
  # Service-fixture changes must not discard compiled integration-test artifacts.
  testArtifacts = craneLib.cargoTest (
    dev
    // {
      cargoArtifacts = devArtifacts;
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
  whatsapp-archive = package "ferret-whatsapp-archive";
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
    whatsapp-archive
    postgres
    cargoArtifacts
    ;
  checks = {
    rust-fmt = craneLib.cargoFmt common;
    rust-clippy = craneLib.cargoClippy (
      dev
      // {
        cargoArtifacts = devArtifacts;
        cargoExtraArgs = "--workspace --all-targets --locked";
        cargoClippyExtraArgs = "-- -D warnings";
      }
    );
    rust-test = craneLib.cargoTest (
      dev
      // services
      // {
        cargoArtifacts = testArtifacts;
        doInstallCargoArtifacts = false;
        cargoExtraArgs = "--workspace --locked";
        nativeBuildInputs = services.nativeBuildInputs ++ [ (lib.getBin pkgs.util-linux) ];
        preBuild = services.preBuild + ''
          test "$(findmnt --noheadings --mountpoint /dev/shm --output FSTYPE)" = tmpfs
          test -z "$(find /dev/shm -mindepth 1 -print -quit)"
          export FERRET_TEST_FILES_ROOT=/dev/shm
          echo "Mounted files fixture: $FERRET_TEST_FILES_ROOT (isolated sandbox tmpfs)"
        '';
      }
    );
    sqlx = craneLib.mkCargoDerivation (
      dev
      // services
      // {
        cargoArtifacts = devArtifacts;
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
