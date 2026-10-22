{
  pkgs,
  craneLib,
  tools,
  postgres,
}:
let
  inherit (pkgs) lib;
  sources = lib.fileset.unions [
    ../Cargo.toml
    ../Cargo.lock
    ../crates
    ../.cargo
    ../.sqlx
  ];
  src = lib.fileset.toSource {
    root = ../.;
    fileset = sources;
  };
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;
  common = {
    inherit src version;
    # Explicit, so the dependency builds never read their generated dummy source during evaluation
    # (import from derivation).
    cargoVendorDir = craneLib.vendorCargoDeps { inherit src; };
    pname = "ferret";
    strictDeps = true;
    SQLX_OFFLINE = "true";
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.cmake
    ];
  };
  # semantic-release bumps the workspace version with every release, which would rebuild all
  # dependencies. Their source pins every workspace member to 0.0.0 instead (cargo's default for a
  # package without a version), in Cargo.toml and Cargo.lock alike, so `--locked` still holds.
  lock = builtins.readFile ../Cargo.lock;
  members = map (package: package.name) (
    builtins.filter (package: !(package ? source)) (builtins.fromTOML lock).package
  );
  lockEntry = memberVersion: name: ''
    name = "${name}"
    version = "${memberVersion}"
  '';
  depsSrc = craneLib.mkDummySrc {
    src = lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.difference sources ../Cargo.lock;
    };
    cargoLock = builtins.toFile "Cargo.lock" (
      builtins.replaceStrings (map (lockEntry version) members) (map (lockEntry "0.0.0") members) lock
    );
    cleanCargoTomlFilter =
      path:
      craneLib.filters.cargoTomlDefault path
      && !(builtins.elem path [
        [
          "package"
          "version"
        ]
        [
          "workspace"
          "package"
          "version"
        ]
      ]);
  };
  deps =
    attrs:
    craneLib.buildDepsOnly (
      builtins.removeAttrs attrs [ "src" ]
      // {
        dummySrc = depsSrc;
        version = "0";
        cargoExtraArgs = "--workspace --locked";
      }
    );
  # Only the shipped packages build on these, so they skip the tests' dev-dependencies.
  cargoArtifacts = deps (common // { doCheck = false; });
  # The checks ship nothing, so they skip release optimization and debug info, which dominate
  # compiling and linking the test binaries. Their deps need artifacts of their own: cargo
  # fingerprints differ per profile and env.
  dev = common // {
    CARGO_PROFILE = "";
    CARGO_PROFILE_DEV_DEBUG = "0";
  };
  devArtifacts = deps dev;
  # Service-fixture changes must not discard compiled integration-test artifacts.
  testArtifacts = craneLib.cargoTest (
    dev
    // {
      cargoArtifacts = devArtifacts;
      cargoExtraArgs = "--workspace --locked --no-run";
    }
  );
  # One cargo invocation, so the workspace crates the binaries share compile once.
  binaries = craneLib.buildPackage (
    common
    // {
      inherit cargoArtifacts;
      pname = "ferret-binaries";
      cargoExtraArgs = "--locked -p ferret-server -p ferret-plugins -p ferret-whatsapp-archive";
      doCheck = false;
    }
  );
  # Each image carries only its own binary, never the combined build.
  binary =
    name:
    pkgs.runCommand "${name}-${version}"
      {
        disallowedReferences = [ binaries ];
        meta.mainProgram = name;
      }
      ''
        install -Dm755 ${binaries}/bin/${name} $out/bin/${name}
      '';
  server = binary "ferret";
  plugins = binary "ferret-plugins";
  whatsapp-archive = binary "ferret-whatsapp-archive";
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
