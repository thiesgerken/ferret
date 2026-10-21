{
  pkgs,
  pnpm,
  nodejs,
}:
let
  manifest = builtins.fromJSON (builtins.readFile ../frontend/package.json);
  src = pkgs.lib.cleanSource ../frontend;
  pnpmDeps = pkgs.fetchPnpmDeps {
    pname = "ferret-frontend";
    inherit (manifest) version;
    inherit src pnpm;
    fetcherVersion = 4;
    hash = pkgs.lib.trim (builtins.readFile ./pnpm-deps-hash);
  };
  common = {
    pname = "ferret-frontend";
    inherit (manifest) version;
    inherit src pnpmDeps;
    nativeBuildInputs = [
      nodejs
      pnpm
      pkgs.pnpmConfigHook
    ];
    NEXT_TELEMETRY_DISABLED = "1";
    CI = "true";
    env.HOME = "/tmp/ferret-frontend-home";
    preConfigure = "mkdir -p $HOME";
    preBuild = ''
      pnpm rebuild
    '';
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
        pnpm run build
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
  inherit frontend pnpmDeps;
  checks = {
    frontend-lint = check "lint" "pnpm run lint";
    frontend-test = check "test" "pnpm run test";
    frontend-build = frontend;
  };
}
