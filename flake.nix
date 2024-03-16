{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    devenv.url = "github:cachix/devenv";
    flake-utils.url = "github:numtide/flake-utils";
  };

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  outputs = { self, nixpkgs, devenv, flake-utils, ... } @ inputs:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        devPackages = with pkgs; [
          git
        ];
        linuxDependencies = with pkgs; [
          xorg.libxcb
        ];
        darwinDependencies = with pkgs.darwin; [
          apple_sdk.frameworks.AppKit
          apple_sdk.frameworks.Foundation
          libobjc
        ];
      in
      {
        devShell = devenv.lib.mkShell {
          inherit inputs pkgs;
          modules = [
            ({ lib, pkgs, ... }: {
              packages = devPackages
              ++ lib.optionals pkgs.stdenv.isLinux linuxDependencies
              ++ lib.optionals pkgs.stdenv.isDarwin darwinDependencies;

              languages.rust.enable = true;
            })
          ];
        };
      }
    );
}
