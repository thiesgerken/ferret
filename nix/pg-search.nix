{ pkgs }:
let
  binaries = builtins.fromJSON (builtins.readFile ./pg-search-binaries.json);
  binary = binaries.platforms.${pkgs.stdenv.hostPlatform.system};
in
pkgs.stdenvNoCC.mkDerivation {
  pname = "pg_search";
  inherit (binaries) version;
  src = pkgs.fetchurl {
    url = "https://github.com/paradedb/paradedb/releases/download/v${binaries.version}/${binary.asset}";
    inherit (binary) hash;
  };
  nativeBuildInputs = [ pkgs.dpkg pkgs.autoPatchelfHook ];
  buildInputs = [ pkgs.stdenv.cc.cc.lib pkgs.openblas ];
  unpackPhase = ''
    runHook preUnpack
    mkdir source
    dpkg-deb --extract "$src" source
    cd source
    runHook postUnpack
  '';
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm755 usr/lib/postgresql/18/lib/pg_search.so "$out/lib/pg_search.so"
    mkdir -p "$out/share/postgresql/extension" "$out/share/doc/pg_search"
    cp usr/share/postgresql/18/extension/* "$out/share/postgresql/extension/"
    cp usr/share/doc/postgresql-18-pg-search/{LICENSE,copyright} "$out/share/doc/pg_search/"
    runHook postInstall
  '';
  meta = {
    description = "ParadeDB search extension for PostgreSQL 18";
    homepage = "https://github.com/paradedb/paradedb";
    license = pkgs.lib.licenses.agpl3Only;
    platforms = [ "aarch64-linux" "x86_64-linux" ];
  };
}
