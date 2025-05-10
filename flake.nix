{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";

    crane.url = "github:ipetkov/crane";
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs =
    inputs@{ self
    , flake-utils
    , nixpkgs
    , rust-overlay
    , crane
    , advisory-db
    , ...
    }:
    flake-utils.lib.eachSystem [ flake-utils.lib.system.x86_64-linux ] (system:
    let
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs { inherit system overlays; };

      rust-custom-toolchain = (pkgs.rust-bin.stable.latest.default.override {
        extensions = [
          "rust-src"
          "rustfmt"
          "llvm-tools-preview"
          "rust-analyzer-preview"
        ];
      });
    in
    rec {
      devShells.default =
        (pkgs.mkShell.override { stdenv = pkgs.llvmPackages.stdenv; }) {
          buildInputs = with pkgs; [ openssl pkg-config ];

          nativeBuildInputs = with pkgs; [
            # get current rust toolchain defaults (this includes clippy and rustfmt)
            rust-custom-toolchain

            cargo-edit
            just
          ];

          # fetch with cli instead of native
          CARGO_NET_GIT_FETCH_WITH_CLI = "true";
          RUST_BACKTRACE = 1;
        };

      default = { };

      checks =
        let
          craneLib =
            (inputs.crane.mkLib pkgs).overrideToolchain rust-custom-toolchain;
          commonArgs = {
            src = ./.;
            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = with pkgs; [ openssl zlib ];
            strictDeps = true;
            version = "0.1.0";
            stdenv = pkgs: pkgs.stdenvAdapters.useMoldLinker pkgs.llvmPackages_15.stdenv;
            CARGO_BUILD_RUSTFLAGS = "-C linker=clang -C link-arg=-fuse-ld=${pkgs.mold}/bin/mold";
          };
          pname = "capnp-checks";

          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            inherit pname;
          });
          build-tests = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts pname;
          });
        in
        {
          inherit build-tests;

          # Run clippy (and deny all warnings) on the crate source,
          # again, reusing the dependency artifacts from above.
          #
          # Note that this is done as a separate derivation so that
          # we can block the CI if there are issues here, but not
          # prevent downstream consumers from building our crate by itself.
          capnp-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            pname = "${pname}-clippy";
            cargoClippyExtraArgs = "-- --deny warnings";
          });

          # Check formatting
          capnp-fmt = craneLib.cargoFmt (commonArgs // {
            pname = "${pname}-fmt";
          });

          # Audit dependencies
          capnp-audit = craneLib.cargoAudit (commonArgs // {
            pname = "${pname}-audit";
            advisory-db = inputs.advisory-db;
            cargoAuditExtraArgs = "--ignore RUSTSEC-2020-0071";
          });

          # Run tests with cargo-nextest
          capnp-nextest = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            pname = "${pname}-nextest";
            partitions = 1;
            partitionType = "count";
          });
        };
    });
}
