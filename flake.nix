{
  description = "Lightweight modifier hint overlay for Niri compositor";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        package = pkgs.callPackage ./package.nix { };
      in
      {
        packages = {
          default = package;
          niri-whichkey = package;
        };

        apps.default = {
          type = "app";
          program = "${package}/bin/niri-whichkey";
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ package ];
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
          ];
        };
      }
    ) // {
      homeManagerModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.programs.niri-whichkey;
          yamlFormat = pkgs.formats.yaml { };
        in
        {
          options.programs.niri-whichkey = {
            enable = lib.mkEnableOption "niri-whichkey modifier hint overlay for Niri";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              description = "The niri-whichkey package to install.";
            };

            settings = lib.mkOption {
              type = yamlFormat.type;
              default = { };
              description = "Configuration written to ~/.config/niri-whichkey/config.yaml";
              example = lib.literalExpression ''
                {
                  threshold_ms = 200;
                  margin_top = 24;
                  margin_right = 24;
                  font_size = 13.0;
                  max_columns = 2;
                }
              '';
            };
          };

          config = lib.mkIf cfg.enable {
            home.packages = [ cfg.package ];

            xdg.configFile."niri-whichkey/config.yaml" = lib.mkIf (cfg.settings != { }) {
              source = yamlFormat.generate "niri-whichkey-config.yaml" cfg.settings;
            };
          };
        };
    };
}
