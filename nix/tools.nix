{
  pkgs,
  craneLib,
  sqlx-src,
}:
let
  nodejs = pkgs.nodejs_24;
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
  inherit nodejs sqlx;
  ci = pkgs.buildEnv {
    name = "ferret-ci-tools";
    paths = with pkgs; [
      bash
      coreutils
      skopeo
      kubernetes-helm
      python3
      git
      curl
      jq
      gh
      cacert
    ];
    pathsToLink = [
      "/bin"
      "/etc/ssl/certs"
    ];
  };
}
