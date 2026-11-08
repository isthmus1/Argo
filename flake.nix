{
  description = "A devShell example";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        gl = pkgs.writeShellScriptBin "gl" ''
          exec nix run --impure github:nix-community/nixGL -- "$@"
        '';
      in {
        devShells.default = with pkgs;
          mkShell {
            buildInputs = [
              openssl
              gl
              clang
              libxkbcommon
              pkg-config
              eza
              fd
              rust-bin.beta.latest.default
            ];

            shellHook = ''
              alias ls=eza
              alias find=fd
            '';
          };
      }
    );
}
