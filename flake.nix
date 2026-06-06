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
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          linuxDependencies = with pkgs; [
            xorg.libxcb
          ];
          buildInputs = with nixpkgs.lib; [ ]
          ++ optionals pkgs.stdenv.isLinux linuxDependencies;
        in
        {
          packages.default = pkgs.rustPlatform.buildRustPackage {
            pname = "clipsy";
            version = "0.1.0";
            src = ./.;
            cargoHash = "sha256-KqSWQNbeWVVQinszNj+a6nxFuo0zf5VLqxnkRx9j1xU=";
            inherit buildInputs;
            nativeBuildInputs = [ ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.python3 ];
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
      ) // (
      let
        mkOptions = { lib, ... }: {
          services.clipsy = {
            enable = lib.mkEnableOption "Enables clipsy service";
          };
        };
        baseConfig = {
          nixpkgs.overlays = [
            self.overlays.default
          ];
        };
      in
      {
        overlays.default = (final: prev: {
          clipsy = self.packages.${final.stdenv.hostPlatform.system}.default;
        });

        nixosModules.darwin = { config, pkgs, lib, ... } @ args: {
          options = mkOptions args;
          config = baseConfig // (lib.mkIf config.services.clipsy.enable {
            launchd.user.agents.clipsy = {
              serviceConfig.ProgramArguments = [
                "${self.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/clipsy"
                "serve"
              ];
              serviceConfig.KeepAlive = true;
              serviceConfig.ProcessType = "Interactive";
            };
          });
        };

        nixosModules.linux = { config, pkgs, lib, ... } @ args: {
          options = mkOptions args;
          config = baseConfig // (lib.mkIf config.services.clipsy.enable {
            systemd.services.clipsy = {
              description = "Clipsy clipboard synchronization service";
              after = [ "network.target" ];
              wantedBy = [ "multi-user.target" ];
              serviceConfig = {
                Type = "simple";
                ExecStart =
                  "${self.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/clipsy serve";
                Restart = "always";
                RestartSec = "30";
              };
            };
          });
        };
      }
    );
}
