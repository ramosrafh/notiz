{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  packages = with pkgs; [
    cargo
    clippy
    cmake
    perl
    pkg-config
    rust-analyzer
    rustc
    rustfmt
  ];

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
    libxkbcommon
    vulkan-loader
    wayland
  ]);

  WINIT_UNIX_BACKEND = "wayland";
  WGPU_BACKEND = "vulkan";
}

