{
  description = "Tool to fix ePub files";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        inherit (pkgs) lib rustPlatform;

        rustSrc = pkgs.rust.packages.stable.rustPlatform.rustLibSrc;

        cargoToml = lib.importTOML ./Cargo.toml;

        src = lib.cleanSource ./.;

        fixepub = rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          inherit src;
          cargoLock.lockFile = ./Cargo.lock;
          doCheck = false;
          buildInputs =
            [
              # Add additional build inputs here
            ]
            ++ lib.optionals pkgs.stdenv.isDarwin [
              # Additional darwin specific inputs can be set here
              pkgs.darwin.libiconv
          ];
          strictDeps = true;
        };

        fixepubNextest = fixepub.overrideAttrs (old: {
          pname = "${old.pname}-nextest";
          nativeBuildInputs = (old.nativeBuildInputs or []) ++ [ pkgs.cargo-nextest ];
          checkPhase = ''
            runHook preCheck
            cargo nextest run --all-targets --workspace
            runHook postCheck
          '';
        });
      in
      {
        checks = {
          inherit fixepub fixepubNextest;
        };

        packages.default = fixepub;

        apps.default =
          (flake-utils.lib.mkApp {
            drv = fixepub;
          }) // {
            meta = {
              description = "Tool to fix ePub files";
            };
          };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ fixepub ];
          RUST_SRC_PATH = "${rustSrc}";
          buildInputs = (with pkgs; [
            cargo
            cargo-nextest
            clippy
            rust-analyzer
            rustfmt
            rustc
          ]);
        };
      });
}
