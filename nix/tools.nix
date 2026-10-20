{
  pkgs,
  craneLib,
  sqlx-src,
}:
let
  manifest = builtins.fromJSON (builtins.readFile ../frontend/package.json);
  pnpmVersion = pkgs.lib.removePrefix "pnpm@" manifest.packageManager;
  nodejs = pkgs.nodejs_24;
  binaries = builtins.fromJSON (builtins.readFile ./pnpm-binaries.json);
  binary = binaries.platforms.${pkgs.stdenv.hostPlatform.system};
  pnpm =
    assert pkgs.lib.assertMsg (binaries.version == pnpmVersion)
      "pnpm binary pins must match frontend/package.json; run the documented scripts/update-nix-deps.py command";
    pkgs.stdenvNoCC.mkDerivation {
      pname = "pnpm";
      version = pnpmVersion;
      src = pkgs.fetchurl {
        url = "https://github.com/pnpm/pnpm/releases/download/v${pnpmVersion}/${binary.asset}";
        inherit (binary) hash;
      };
      unpackPhase = ''
        runHook preUnpack
        mkdir source
        tar -xzf "$src" -C source
        cd source
        runHook postUnpack
      '';
      dontBuild = true;
      nativeBuildInputs = [
        pkgs.makeWrapper
      ]
      ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.autoPatchelfHook ];
      buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.stdenv.cc.cc.lib ];
      installPhase = ''
        runHook preInstall
        mkdir -p "$out/libexec/pnpm" "$out/bin"
        install -m755 pnpm "$out/libexec/pnpm/pnpm"
        cp -R dist "$out/libexec/pnpm/"
        makeWrapper "$out/libexec/pnpm/pnpm" "$out/bin/pnpm" \
          --prefix PATH : ${pkgs.lib.makeBinPath [ nodejs ]}
        makeWrapper "$out/bin/pnpm" "$out/bin/pnpx" --add-flags dlx
        runHook postInstall
      '';
      doInstallCheck = true;
      installCheckPhase = ''
        pushd "$TMPDIR"
        test "$("$out/bin/pnpm" --version)" = "${pnpmVersion}"
        popd
      '';
      passthru = {
        nodejs-slim = nodejs;
        majorVersion = pkgs.lib.versions.major pnpmVersion;
      };
    };
  sqlx = craneLib.buildPackage {
    pname = "sqlx-cli";
    version = (builtins.fromTOML (builtins.readFile (sqlx-src + "/Cargo.toml"))).package.version;
    src = sqlx-src;
    cargoExtraArgs = "--locked -p sqlx-cli --no-default-features --features rustls,postgres";
    doCheck = false;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.openssl ];
  };
in
{
  inherit nodejs pnpm sqlx;
}
