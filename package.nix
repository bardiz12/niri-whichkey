{ lib, rustPlatform, pkg-config, wayland, libxkbcommon }:

rustPlatform.buildRustPackage {
  pname = "niri-whichkey";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let base = baseNameOf path;
      in base != "target";
  };

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs = [
    wayland
    libxkbcommon
  ];

  meta = with lib; {
    description = "Lightweight modifier hint overlay for Niri compositor";
    mainProgram = "niri-whichkey";
    platforms = platforms.linux;
  };
}
