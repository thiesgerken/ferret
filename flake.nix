{
  description = "Ferret packages, checks, runtime images, and developer tooling";

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
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
      sqlx-src,
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
              rust
              sqlx-src
              ;
          };
          backend = import ./nix/rust.nix {
            inherit pkgs craneLib tools postgres;
          };
          frontend = import ./nix/frontend.nix {
            inherit pkgs;
            inherit (tools) pnpm nodejs;
          };
          images = import ./nix/images.nix {
            inherit pkgs;
            inherit (backend) server plugins whatsapp-archive;
            inherit (frontend) frontend;
          };
        in
        {
          inherit
            pkgs
            rust
            tools
            backend
            frontend
            images
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
          inherit (p.tools) pnpm sqlx;
          ci-tools = p.tools.ci;
          release-tools = p.tools.release;
        }
        // nixpkgs.lib.optionalAttrs (builtins.elem system linuxSystems) (
          {
            inherit (p.backend) server plugins whatsapp-archive;
            inherit (p.frontend) frontend pnpmDeps;
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
      imageBuilds = forLinux (system: project.${system}.images);
      ciBuilds = forSystems (
        system:
        let
          linux = if system == "aarch64-darwin" then "aarch64-linux" else system;
          p = project.${linux};
        in
        self.checks.${linux}
        // {
          inherit (p.backend) server plugins whatsapp-archive;
          inherit (p.frontend) frontend;
          tools = project.${system}.tools.ci;
          release-tools = project.${system}.tools.release;
        }
        // self.imageBuilds.${linux}
      );
      devShells = forSystems (
        system:
        let
          inherit (project.${system}) pkgs rust tools;
        in
        {
          default = pkgs.mkShell {
            packages = [
              rust
              tools.nodejs
              tools.pnpm
              pkgs.pkg-config
              pkgs.git
              pkgs.curl
              pkgs.jq
              pkgs.kubernetes-helm
              pkgs.skopeo
              tools.sqlx
            ];
          };
        }
      );
    };
}
