{
  description = "Ferret packages, checks, runtime images, and developer tooling";

  # Release CI pushes its x86_64-linux builds here.
  nixConfig = {
    extra-substituters = [ "https://thiesgerken.cachix.org" ];
    extra-trusted-public-keys = [
      "thiesgerken.cachix.org-1:SpR6r/0v3OYrmAITqLiN+XxzJLKZg+zT4nzs4GlpTGE="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    sqlx-src = {
      url = "tarball+https://static.crates.io/crates/sqlx-cli/sqlx-cli-0.9.0.crate";
      flake = false;
    };
    # The CLI runs from this input (`nix run --inputs-from . presswerk`), so CLI and declaration
    # schema always move together. Its own nixpkgs stays: that is the combination upstream tests.
    presswerk.url = "github:jkuball/presswerk/v0.16.0";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
      sqlx-src,
      presswerk,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forSystems = nixpkgs.lib.genAttrs systems;
      linuxSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forLinux = nixpkgs.lib.genAttrs linuxSystems;
      linuxFor = system: if system == "aarch64-darwin" then "aarch64-linux" else system;
      project = forSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          postgres = pkgs.postgresql_18.withPackages (ps: [
            (import ./nix/pg-search.nix { inherit pkgs; })
            ps.pgvector
          ]);
          rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain rust;
          tools = import ./nix/tools.nix {
            inherit
              pkgs
              craneLib
              sqlx-src
              ;
          };
          backend = import ./nix/rust.nix {
            inherit pkgs craneLib tools postgres;
          };
          frontend = import ./nix/frontend.nix {
            inherit pkgs;
            inherit (tools) nodejs;
          };
          images = import ./nix/images.nix {
            inherit pkgs;
            inherit (backend) server plugins whatsapp-archive;
            inherit (frontend) frontend;
          };
          # The version comes from the committed Chart.yaml, which the release commit bumps.
          chart =
            pkgs.runCommand "ferret-chart.tgz"
              {
                nativeBuildInputs = [ pkgs.kubernetes-helm ];
                src = ./charts/ferret;
              }
              ''
                helm package "$src" --destination .
                mv ferret-*.tgz "$out"
              '';
        in
        {
          inherit
            pkgs
            rust
            tools
            backend
            frontend
            images
            chart
            ;
        }
      );
    in
    {
      packages = forSystems (
        system:
        let
          p = project.${system};
        in
        {
          inherit (p.tools) sqlx;
          ci-tools = p.tools.ci;
        }
        // nixpkgs.lib.optionalAttrs (builtins.elem system linuxSystems) (
          {
            inherit (p.backend) server plugins whatsapp-archive;
            inherit (p.frontend) frontend;
            inherit (p) chart;
            default = p.backend.server;
          }
          // p.images
        )
      );
      checks = forLinux (
        system:
        let
          p = project.${system};
        in
        p.backend.checks
        // p.frontend.checks
        // {
          helm =
            p.pkgs.runCommand "ferret-helm"
              {
                nativeBuildInputs = [ p.pkgs.kubernetes-helm ];
                src = nixpkgs.lib.fileset.toSource {
                  root = ./.;
                  fileset = nixpkgs.lib.fileset.unions [
                    ./charts
                    ./scripts/check-chart.sh
                  ];
                };
              }
              ''
                cp -R "$src" source
                chmod -R u+w source
                cd source
                bash scripts/check-chart.sh
                touch "$out"
              '';
        }
      );
      ciBuilds = forSystems (
        system:
        let
          linux = linuxFor system;
          p = project.${linux};
        in
        self.checks.${linux}
        // {
          inherit (p.backend) server plugins whatsapp-archive;
          inherit (p.frontend) frontend;
          inherit (p) chart;
          tools = project.${system}.tools.ci;
        }
        // p.images
        # The release jobs run there and substitute the CLI from our cache, not a third party's.
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          presswerk = presswerk.packages.${system}.default;
        }
      );
      devShells = forSystems (
        system:
        let
          inherit (project.${system}) pkgs rust tools;
          linux = linuxFor system;
        in
        {
          default = pkgs.mkShell {
            packages = [
              rust
              tools.nodejs
              pkgs.pkg-config
              pkgs.git
              pkgs.curl
              pkgs.python3
              pkgs.jq
              pkgs.kubernetes-helm
              pkgs.skopeo
              tools.sqlx
            ];
            shellHook = ''
              printf '🦦 \033[1m\033[38;2;243;156;18mf\033[38;2;233;136;23me\033[38;2;223;116;28mr\033[38;2;212;97;33mr\033[38;2;202;77;38me\033[38;2;192;57;43mt\033[0m  \033[2m%s\033[0m\n' 'Rust ${rust.version} · node ${tools.nodejs.version} · 📖 docs/development.md#tests-and-checks'
              printf '  \033[32m%s\033[0m \033[2m·\033[0m \033[32m%s\033[0m\n' 'cargo clippy --workspace --all-targets' 'cargo test --workspace (Compose: paradedb + dovecot)'
              printf '  \033[32m%s\033[0m \033[2m·\033[0m \033[32m%s\033[0m\n' 'npm --prefix frontend run dev' 'nix build .#checks.${linux}.<check>'
            '';
          };
        }
      );
      presswerk.default = presswerk.lib.mkRelease {
        sourceRoot = self.outPath;
        versionFiles = {
          cargoWorkspace.ferret = { };
          json = {
            frontend = {
              path = "frontend/package.json";
              key = "version";
            };
            frontend-lock = {
              path = "frontend/package-lock.json";
              key = "version";
            };
            frontend-lock-root = {
              path = "frontend/package-lock.json";
              key = ''packages."".version'';
            };
          };
          regex = {
            chart = {
              path = "charts/ferret/Chart.yaml";
              pattern = ''^version: (?<version>\S+)$'';
            };
            chart-app = {
              path = "charts/ferret/Chart.yaml";
              pattern = ''^appVersion: "(?<version>[^"]+)"$'';
            };
          };
        };
        analyzer.gitmoji = {
          enable = true;
          # Refactorings and CI changes ship in the images, so they release a patch like fixes do.
          catalog.recycle.semver = "patch";
          catalog.construction_worker.semver = "patch";
        };
        # The format python-semantic-release wrote, so the existing sections stay recognized.
        changelog.heading = "## v{version} ({date})";
        publishers =
          let
            # The packages are public, so `plan` and `prepare` check the previous release anonymously.
            ghcr = {
              public = true;
              username.secret = "GITHUB_ACTOR";
              password.secret = "GITHUB_TOKEN";
            };
            artifacts = self.packages.x86_64-linux;
          in
          {
            oci-image =
              nixpkgs.lib.mapAttrs
                (
                  name: image:
                  ghcr
                  // {
                    repository = "ghcr.io/thiesgerken/${name}";
                    platforms."linux/amd64" = image;
                    aliases = [ "latest" ];
                  }
                )
                {
                  ferret = artifacts.server-image;
                  ferret-plugins = artifacts.plugins-image;
                  ferret-whatsapp-archive = artifacts.whatsapp-archive-image;
                  ferret-frontend = artifacts.frontend-image;
                };
            oci-helm.ferret = ghcr // {
              repository = "ghcr.io/thiesgerken/charts/ferret";
              inherit (artifacts) chart;
            };
            github-release.default = {
              repository = "thiesgerken/ferret";
              body = ''
                {{ notes }}

                ## Docker Images

                ```
                docker pull ghcr.io/thiesgerken/ferret:{{ version }}
                docker pull ghcr.io/thiesgerken/ferret-plugins:{{ version }}
                docker pull ghcr.io/thiesgerken/ferret-whatsapp-archive:{{ version }}
                docker pull ghcr.io/thiesgerken/ferret-frontend:{{ version }}
                ```

                ## Helm Chart

                ```
                helm install ferret oci://ghcr.io/thiesgerken/charts/ferret --version {{ version }}
                ```
              '';
            };
          };
      };
    };
}
