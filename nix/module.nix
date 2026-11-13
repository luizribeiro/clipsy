{ lib }:
let
  inherit (lib) mkEnableOption mkOption types;

  nodeId = types.strMatching "[0-9a-f]{64}" // {
    description = "clipsy node id (64 lowercase hex characters, as printed by `clipsy id`)";
  };

  absolutePath = types.addCheck types.str (lib.hasPrefix "/") // {
    description = "absolute path";
  };

  client = types.submodule {
    options = {
      id = mkOption {
        type = nodeId;
        description = "Node id the client prints with `clipsy id`.";
      };
      read = mkOption {
        type = types.bool;
        default = false;
        description = "Whether the client may read this machine's clipboard. Off by default, since the clipboard often holds secrets.";
      };
      readConcealed = mkOption {
        type = types.bool;
        default = false;
        description = "Whether the client may read entries that password managers mark as concealed. Needs `read` too.";
      };
      write = mkOption {
        type = types.bool;
        default = true;
        description = "Whether the client may write this machine's clipboard.";
      };
    };
  };
in
{
  options = {
    options.services.clipsy = {
      enable = mkEnableOption "the clipsy server, which shares this machine's clipboard with allowed clients";
      allowedClients = mkOption {
        type = types.attrsOf client;
        default = { };
        description = "Clients allowed to use this server, keyed by a name used in logs.";
      };
    };

    options.programs.clipsy = {
      servers = mkOption {
        type = types.attrsOf nodeId;
        default = { };
        description = "Servers this machine can use, mapping names to node ids.";
      };
      defaultServer = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Server used when neither --server nor CLIPSY_SERVER is given.";
      };
      relayUrl = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "https://relay.example.com";
        description = "iroh relay to use instead of n0's public relays and DNS address lookup.";
      };
      keyFile = mkOption {
        type = types.nullOr absolutePath;
        default = null;
        description = "Path to this machine's secret key, outside the Nix store, such as a sops-nix secret. Defaults to ~/.local/state/clipsy/secret.key.";
      };
    };
  };

  validation = { config, ... }:
    let
      server = config.services.clipsy;
      client = config.programs.clipsy;
      clientIds = lib.mapAttrsToList (_: c: c.id) server.allowedClients;
    in
    {
      assertions = [
        {
          assertion = client.defaultServer == null || client.servers ? ${client.defaultServer};
          message = "programs.clipsy.defaultServer must name one of programs.clipsy.servers";
        }
        {
          assertion = client.keyFile == null || !lib.hasPrefix builtins.storeDir client.keyFile;
          message = "programs.clipsy.keyFile must not be in the Nix store, where every user can read it";
        }
        {
          assertion = lib.length clientIds == lib.length (lib.unique clientIds);
          message = "services.clipsy.allowedClients lists the same node id under two names";
        }
      ];
      warnings = lib.optional (server.enable && server.allowedClients == { })
        "services.clipsy.enable is set but services.clipsy.allowedClients is empty, so no client can connect";
    };

  render = pkgs: config:
    (pkgs.formats.toml { }).generate "clipsy.toml" (lib.filterAttrs (_: value: value != null && value != { }) {
      relay_url = config.programs.clipsy.relayUrl;
      key_file = config.programs.clipsy.keyFile;
      default_server = config.programs.clipsy.defaultServer;
      servers = config.programs.clipsy.servers;
      clients = lib.mapAttrs
        (_: client: {
          inherit (client) id read write;
          read_concealed = client.readConcealed;
        })
        config.services.clipsy.allowedClients;
    });
}
