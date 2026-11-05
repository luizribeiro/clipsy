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
    let
      inherit (nixpkgs) lib;

      clipsyNix = import ./nix/module.nix { inherit lib; };

      clipsyModule = serviceConfig: { config, pkgs, ... }:
        let
          clipsy = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        in
        {
          imports = [ clipsyNix.options clipsyNix.validation ];
          config = lib.mkMerge [
            {
              nixpkgs.overlays = [ self.overlays.default ];
              environment.etc."clipsy/config.toml".source = clipsyNix.render pkgs config;
            }
            (lib.mkIf config.services.clipsy.enable (serviceConfig "${clipsy}/bin/clipsy"))
          ];
        };

      evalClipsy = settings: (lib.evalModules {
        modules = [
          clipsyNix.options
          clipsyNix.validation
          {
            options.assertions = lib.mkOption { type = lib.types.listOf lib.types.unspecified; };
            options.warnings = lib.mkOption { type = lib.types.listOf lib.types.str; };
          }
          settings
        ];
      }).config;

      failedAssertions = settings:
        map (a: a.message) (lib.filter (a: !a.assertion) (evalClipsy settings).assertions);

      devboxId = "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c";
      sandboxId = "8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394";

      typicalSettings = {
        services.clipsy = {
          enable = true;
          allowedClients = {
            devbox.id = devboxId;
            sandbox = { id = sandboxId; read = false; };
          };
        };
        programs.clipsy = {
          servers.laptop = devboxId;
          defaultServer = "laptop";
          relayUrl = "https://relay.example.com";
          keyFile = "/run/secrets/clipsy-key";
        };
      };

      invalidSettings = {
        unknownDefaultServer.programs.clipsy = { servers.laptop = devboxId; defaultServer = "macbook"; };
        keyFileInStore.programs.clipsy.keyFile = "${builtins.storeDir}/abc-clipsy-key";
        duplicateClientIds.services.clipsy.allowedClients = { devbox.id = devboxId; old-devbox.id = devboxId; };
      };

      rejectsBadOptionValue = settings:
        !(builtins.tryEval (builtins.deepSeq (evalClipsy settings).programs.clipsy (evalClipsy settings).services.clipsy)).success;
    in
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          linuxDependencies = with pkgs; [
            libxcb
          ];
          buildInputs = with nixpkgs.lib; [ ]
          ++ optionals pkgs.stdenv.hostPlatform.isLinux linuxDependencies;
        in
        {
          checks.config =
            assert failedAssertions typicalSettings == [ ];
            assert (evalClipsy typicalSettings).warnings == [ ];
            assert lib.all (settings: failedAssertions settings != [ ]) (lib.attrValues invalidSettings);
            assert (evalClipsy { services.clipsy.enable = true; }).warnings != [ ];
            assert rejectsBadOptionValue { programs.clipsy.servers.laptop = "not-a-node-id"; };
            assert rejectsBadOptionValue { programs.clipsy.keyFile = "secret.key"; };
            pkgs.runCommand "clipsy-config-check"
              {
                full = clipsyNix.render pkgs (evalClipsy typicalSettings);
                withDefaultKey = clipsyNix.render pkgs (evalClipsy (lib.recursiveUpdate typicalSettings {
                  programs.clipsy.keyFile = null;
                }));
              }
              ''
                HOME=$TMPDIR ${self.packages.${system}.default}/bin/clipsy --config $withDefaultKey id > /dev/null
                grep -qx 'key_file = "/run/secrets/clipsy-key"' $full
                touch $out
              '';

          packages.default = pkgs.rustPlatform.buildRustPackage {
            pname = "clipsy";
            version = "0.1.0";
            src = ./.;
            cargoHash = "sha256-dQUyThZxNfTzCxq0s9VZe631i1ijgE1bl4lTXMaxgs4=";
            inherit buildInputs;
            # Binding an iroh endpoint segfaults inside the macOS build sandbox.
            cargoTestFlags = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ "--lib" ];
            checkFlags = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              "--skip=net::tests::binds_with_the_given_identity"
            ];
            nativeBuildInputs = [ ]
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.python3 ];
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
      ) // {
        overlays.default = (final: prev: {
          clipsy = self.packages.${final.stdenv.hostPlatform.system}.default;
        });

        nixosModules.darwin = clipsyModule (clipsy: {
          launchd.user.agents.clipsy.serviceConfig = {
            ProgramArguments = [ clipsy "serve" ];
            KeepAlive = true;
            ProcessType = "Interactive";
          };
        });

        nixosModules.linux = clipsyModule (clipsy: {
          systemd.services.clipsy = {
            description = "Clipsy clipboard synchronization service";
            after = [ "network.target" ];
            wantedBy = [ "multi-user.target" ];
            serviceConfig = {
              Type = "simple";
              ExecStart = "${clipsy} serve";
              Restart = "always";
              RestartSec = "30";
            };
          };
        });
      };
}
