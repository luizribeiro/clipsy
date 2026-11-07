# Nix

The flake (`github:luizribeiro/clipsy`) has modules for NixOS
(`clipsy.nixosModules.linux`) and nix-darwin
(`clipsy.nixosModules.darwin`), plus `clipsy.overlays.default` to put
`clipsy` in `pkgs` (the nvim and tmux plugins need it on your `PATH`).

```nix
# on a server
services.clipsy.enable = true;
services.clipsy.allowedClients = {
  devbox = { id = "<node id>"; read = true; };
  ci.id = "<node id>";   # write-only
};

# on a client
programs.clipsy.servers.laptop = "<node id>";
programs.clipsy.defaultServer = "laptop";

# optional, on every machine
programs.clipsy.relayUrl = "https://relay.example.com";
programs.clipsy.keyFile = "/run/secrets/clipsy";
```

On Linux the server runs as a systemd user service tied to
`graphical-session.target`, so your session has to export `DISPLAY` or
`WAYLAND_DISPLAY` to the user manager (GNOME-based sessions do). On macOS
it's a launchd user agent.
