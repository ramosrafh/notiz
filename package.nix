{
  lib,
  rustPlatform,
  cmake,
  makeWrapper,
  perl,
  pkg-config,
  libxkbcommon,
  vulkan-loader,
  wayland,
}:

rustPlatform.buildRustPackage {
  pname = "notiz";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let name = baseNameOf path;
      in name != ".git" && name != "target";
  };

  cargoLock.lockFile = ./Cargo.lock;
  cargoBuildFlags = [ "-p" "notiz" ];
  cargoTestFlags = [ "--workspace" ];

  nativeBuildInputs = [ cmake makeWrapper perl pkg-config ];
  buildInputs = [ libxkbcommon vulkan-loader wayland ];

  postInstall = ''
    install -Dm644 assets/notiz.svg \
      "$out/share/icons/hicolor/scalable/apps/notiz.svg"
    install -Dm644 packaging/notiz.desktop \
      "$out/share/applications/notiz.desktop"

    wrapProgram "$out/bin/notiz" \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath [ libxkbcommon vulkan-loader wayland ]} \
      --set WINIT_UNIX_BACKEND wayland \
      --set WGPU_BACKEND vulkan
  '';

  meta = {
    description = "Native Markdown knowledge base and graph for Wayland";
    license = lib.licenses.mit;
    mainProgram = "notiz";
    platforms = lib.platforms.linux;
  };
}

