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
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; config.allowUnfree = true; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "rustfmt" "clippy" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Narrow source for cargo work (build, clippy, fmt, dep cache). Keeping
        # this filter unchanged means buildDepsOnly's cache is not invalidated
        # by edits to .github/workflows/*.yml.
        cargoSrc = craneLib.cleanCargoSource ./.;

        # Wider source for tests only: cargo sources plus .github/workflows/*.yml
        # so the workflow-validation tests in crates/aivcs-core
        # (eval_workflow.rs, ci_workflow.rs) can read those YAMLs at runtime.
        # Without this, those tests panic inside the nix sandbox.
        #
        # cleanSourceWith requires every ancestor directory of an included file
        # to also pass the filter, so the `.github` and `.github/workflows`
        # dirs are matched explicitly — not just the YAML leaves. The leaf
        # match restricts to regular `.yml` files so the filter never picks up
        # symlinks, sockets, or stray `.yaml`/swap files in that path.
        testSrc = pkgs.lib.cleanSourceWith {
          src = ./.;
          name = "test-source";
          filter = path: type:
            let p = toString path; in
            (craneLib.filterCargoSources path type)
            || (type == "directory"
                && (pkgs.lib.hasSuffix "/.github" p
                    || pkgs.lib.hasSuffix "/.github/workflows" p))
            || (type == "regular"
                && pkgs.lib.hasInfix "/.github/workflows/" p
                && pkgs.lib.hasSuffix ".yml" p);
        };

        # Common args for crane builds
        commonArgs = {
          src = cargoSrc;
          strictDeps = true;
          buildInputs = with pkgs; [
            openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];
          nativeBuildInputs = with pkgs; [
            pkg-config
            git
          ];
        };

        # Build workspace deps first (for caching)
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # Build the full workspace. Tests are run separately by the `tests`
        # check (cargoNextest with `testSrc` that includes .github/workflows/);
        # disabling `doCheck` here avoids a duplicate `cargo test` against
        # `cargoSrc`, which lacks the workflow YAML files and would fail the
        # workflow-validation tests.
        workspace = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          doCheck = false;
        });
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
        };

        packages = {
          default = workspace;

          aivcs = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p aivcs-cli";
          });
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = with pkgs; [
            # Rust extras
            cargo-watch
            cargo-nextest

            # SurrealDB
            surrealdb

            # Tools
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
            echo ""
          '';
        };
      }
    );
}
