{
  pkgs,
  nodejs,
}:
let
  manifest = builtins.fromJSON (builtins.readFile ../frontend/package.json);
  src = pkgs.lib.cleanSource ../frontend;
  # importNpmLock fetches each package from the integrity hashes in package-lock.json,
  # so dependency updates need no separate fixed-output hash.
  npmDeps = pkgs.importNpmLock { npmRoot = ../frontend; };
  common = {
    pname = "ferret-frontend";
    inherit (manifest) version;
    inherit src npmDeps;
    nativeBuildInputs = [
      nodejs
      pkgs.importNpmLock.npmConfigHook
    ];
    NEXT_TELEMETRY_DISABLED = "1";
    CI = "true";
  };
  check =
    name: command:
    pkgs.stdenvNoCC.mkDerivation (
      common
      // {
        pname = "ferret-frontend-${name}";
        buildPhase = ''
          runHook preBuild
          ${command}
          runHook postBuild
        '';
        installPhase = "touch $out";
      }
    );
  frontend = pkgs.stdenvNoCC.mkDerivation (
    common
    // {
      buildPhase = ''
        runHook preBuild
        npm run build
        runHook postBuild
      '';
      installPhase = ''
        mkdir -p "$out"
        cp -R out/. "$out/"
      '';
    }
  );
in
{
  inherit frontend;
  checks = {
    frontend-lint = check "lint" "npm run lint";
    frontend-test = check "test" "npm run test";
    frontend-build = frontend;
  };
}
