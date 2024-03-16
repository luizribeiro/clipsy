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
        linuxDependencies = with pkgs; [
          xorg.libxcb
        ];
        darwinDependencies = with pkgs.darwin; [
          apple_sdk.frameworks.AppKit
          apple_sdk.frameworks.Foundation
          libobjc
        ];
        buildInputs = []
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxDependencies
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin darwinDependencies;
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage rec {
          pname = "clipsy";
          version = "0.1.0";
          src = ./.;
          cargoSha256 = "sha256-sazl9/CAImYLvokBiKZ+jzyp5Q8O6tF3z7tcUWSlaAA=";
          inherit buildInputs;
        };
        devShell = devenv.lib.mkShell {
          inherit inputs pkgs;
          modules = [
            ({ lib, pkgs, ... }: {
              packages = buildInputs ++ (with pkgs; [
                git
              ]);
              languages.rust.enable = true;
            })
          ];
        };
      }
    );
}
