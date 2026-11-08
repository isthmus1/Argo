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
              # Runtime libraries winit/wgpu dlopen() (wayland-dlopen / x11-dl /
              # the Vulkan loader), so they never appear as DT_NEEDED.
              wayland
              vulkan-loader
              libX11
              libXcursor
              libXrandr
              libXi
              libxcb
              libXrender
              pkg-config
              eza
              fd
              rust-bin.beta.latest.default
            ];

            shellHook = ''
              alias ls=eza
              alias find=fd
              # mkShell does not put buildInputs on the dynamic loader path, and the
              # libraries above are dlopen()ed rather than linked (DT_NEEDED).
              export LD_LIBRARY_PATH="${lib.makeLibraryPath [
                wayland
                vulkan-loader
                libX11
                libXcursor
                libXrandr
                libXi
                libxcb
                libXrender
              ]}:$LD_LIBRARY_PATH"
            '';
          };
      }
    );
}
