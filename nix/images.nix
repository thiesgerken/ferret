{
  pkgs,
  server,
  plugins,
  frontend,
}:
let
  identity = pkgs.runCommand "ferret-runtime-identity" { } ''
    mkdir -p "$out/etc"
    echo 'ferret:x:65532:65532:Ferret:/tmp:/sbin/nologin' > "$out/etc/passwd"
    echo 'ferret:x:65532:' > "$out/etc/group"
  '';
  image =
    name: contents: config:
    pkgs.dockerTools.buildLayeredImage {
      inherit name;
      tag = "local";
      contents = contents ++ [ identity ];
      extraCommands = ''
        mkdir -p tmp
        chmod 1777 tmp
      '';
      config = {
        User = "65532:65532";
        WorkingDir = "/tmp";
        Env = [
          "HOME=/tmp"
          "TMPDIR=/tmp"
        ];
      }
      // config;
    };
  tlsEnv = [
    "HOME=/tmp"
    "TMPDIR=/tmp"
    "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
    "SSL_CERT_DIR=${pkgs.cacert}/etc/ssl/certs"
    "GIT_SSL_CAINFO=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
  ];
  frontendRoutes = pkgs.writeText "ferret-frontend-routes.conf" (
    builtins.replaceStrings [ "listen 80;" "/usr/share/nginx/html" ] [ "listen 8080;" "${frontend}" ] (
      builtins.readFile ../frontend/nginx.conf
    )
  );
  nginxConfig = pkgs.writeText "ferret-nginx.conf" ''
    pid /tmp/nginx.pid;
    error_log /dev/stderr;
    events { worker_connections 1024; }
    http {
      include ${pkgs.nginx}/conf/mime.types;
      access_log /dev/stdout;
      client_body_temp_path /tmp/client_body;
      proxy_temp_path /tmp/proxy;
      fastcgi_temp_path /tmp/fastcgi;
      uwsgi_temp_path /tmp/uwsgi;
      scgi_temp_path /tmp/scgi;
      include ${frontendRoutes};
    }
  '';
in
{
  server-image = image "ferret" [ server pkgs.cacert ] {
    Cmd = [ "${server}/bin/ferret" ];
    Env = tlsEnv;
    ExposedPorts = {
      "8080/tcp" = { };
      "8081/tcp" = { };
    };
  };
  plugins-image = image "ferret-plugins" [ plugins pkgs.cacert pkgs.gitMinimal ] {
    Cmd = [ "${plugins}/bin/ferret-plugins" ];
    Env = tlsEnv ++ [ "PATH=${pkgs.gitMinimal}/bin" ];
  };
  frontend-image = image "ferret-frontend" [ pkgs.nginx frontend ] {
    Cmd = [
      "${pkgs.nginx}/bin/nginx"
      "-c"
      "${nginxConfig}"
      "-g"
      "daemon off;"
    ];
    ExposedPorts."8080/tcp" = { };
  };
}
