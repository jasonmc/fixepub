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

        packageArgs = {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          inherit src;
          cargoLock.lockFile = ./Cargo.lock;
          doCheck = false;
          buildInputs =
            [
              # Add additional build inputs here
            ]
            ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              # Additional darwin specific inputs can be set here
              pkgs.darwin.libiconv
          ];
          strictDeps = true;
        };

        fixepub = rustPlatform.buildRustPackage packageArgs;

        fixepubStatic = pkgs.pkgsStatic.rustPlatform.buildRustPackage (packageArgs // {
          doCheck = true;
          buildInputs = [];
          allowedReferences = [];
        });

        fixepubNextest = fixepub.overrideAttrs (old: {
          pname = "${old.pname}-nextest";
          doCheck = true;
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

        packages = { default = fixepub; }
          // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            static = fixepubStatic;
          };

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
