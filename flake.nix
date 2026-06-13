{
  description = "AIVCS - AI Version Control System";

  nixConfig = {
    extra-substituters = [ "https://nix-cache.stevedores.org" ];
    extra-trusted-public-keys = [
      "stevedores-1:ZEtb+wHYNR/LDmMDhF3/EpRZDNma8exY2b1TGZ6uS2A="
      # Legacy key — kept trusted for any artifacts already pushed under
      # this name. Can be removed once the cache is re-signed under stevedores-1.
      "stevedores-cache-1:bXLxkipycRWproIJnk8pPWNFdgVfeV+I2mJXCoW4/ag="
    ];
  };

  # NOTE: Inputs are pinned to exact commits via flake.lock (committed to repo).
  # Run `nix flake update` to bump, and review the lock diff before merging.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane.url = "github:ipetkov/crane";

    nixos-wsl = {
      url = "github:nix-community/NixOS-WSL";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs@{ self, nixpkgs, flake-utils, rust-overlay, crane, nixos-wsl, ... }:
    let
      mkSystemPackages = system:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs { inherit system overlays; config.allowUnfree = true; };

          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "rustfmt" "clippy" ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          cargoSrc = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let p = toString path; in
              (pkgs.lib.hasSuffix ".pem" p)
              || (type == "directory" && pkgs.lib.hasSuffix "/keys" p)
              || (craneLib.filterCargoSources path type);
          };

          testSrc = pkgs.lib.cleanSourceWith {
            src = ./.;
            name = "test-source";
            filter = path: type:
              let p = toString path; in
              (craneLib.filterCargoSources path type)
              || (pkgs.lib.hasSuffix ".pem" p)
              || (type == "directory" && pkgs.lib.hasSuffix "/keys" p)
              || (type == "directory"
                  && (pkgs.lib.hasSuffix "/.github" p
                      || pkgs.lib.hasSuffix "/.github/workflows" p))
              || (type == "regular"
                  && pkgs.lib.hasInfix "/.github/workflows/" p
                  && pkgs.lib.hasSuffix ".yml" p);
          };

          commonArgs = {
            src = cargoSrc;
            strictDeps = true;
            buildInputs = with pkgs; [
              openssl
            ];
            nativeBuildInputs = with pkgs; [
              pkg-config
              git
            ];
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          workspace = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
          });

          aivcs = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p aivcs-cli";
            pname = "aivcs";
            meta.mainProgram = "aivcs";
          });

          aivcsd = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p aivcsd";
            pname = "aivcsd";
            meta.mainProgram = "aivcsd";
          });
        in
        {
          inherit pkgs craneLib commonArgs cargoArtifacts cargoSrc testSrc workspace aivcs aivcsd;
        };

      linuxPackages = mkSystemPackages "x86_64-linux";
    in
    flake-utils.lib.eachDefaultSystem (system:
      let
        inherit (mkSystemPackages system) pkgs craneLib commonArgs cargoArtifacts cargoSrc testSrc workspace aivcs aivcsd;
        wslChecks =
          if system == "x86_64-linux" then {
            aivcs-wsl = self.nixosConfigurations.aivcs-wsl.config.system.build.toplevel;
            aivcs-wsl-e2e = import ./nix/tests/aivcs-wsl-e2e.nix {
              inherit pkgs;
              aivcsPackage = aivcs;
              aivcsdPackage = aivcsd;
            };
          } else { };
      in
      {
        checks = {
          inherit workspace;

          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });

          fmt = craneLib.cargoFmt {
            src = cargoSrc;
          };

          tests = craneLib.cargoNextest (commonArgs // {
            src = testSrc;
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
          });
        } // wslChecks;

        packages = {
          default = workspace;
          inherit aivcs aivcsd;
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = with pkgs; [
            cargo-watch
            cargo-nextest
            surrealdb
            just
            git
          ];

          RUST_BACKTRACE = "1";

          shellHook = ''
            echo "AIVCS Development Environment"
            echo ""
            echo "Commands:"
            echo "  cargo test --workspace        # Run all tests"
            echo "  cargo run -p aivcs-cli        # Run CLI"
            echo "  surreal start memory           # Start SurrealDB (in-memory)"
            echo "  nix build .#nixosConfigurations.aivcs-wsl.config.system.build.tarballBuilder"
            echo ""
          '';
        };
      }
    )
    // {
      nixosConfigurations.aivcs-wsl = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        specialArgs = {
          inherit inputs;
          aivcsPackage = linuxPackages.aivcs;
          aivcsdPackage = linuxPackages.aivcsd;
        };
        modules = [
          nixos-wsl.nixosModules.default
          ./nix/nixos/aivcs-wsl.nix
        ];
      };
    };
}
