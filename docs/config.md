# Config

One TOML file. clipsy uses `--config` (or `CLIPSY_CONFIG`) if you pass
one, otherwise the first of `~/.config/clipsy/config.toml` (honoring
`XDG_CONFIG_HOME`) and `/etc/clipsy/config.toml` that exists. They aren't
merged, so a user config hides the system one.

The server re-reads its config on every connection. To revoke a client,
delete its entry. No restart needed.

## Your own relay

By default clipsy uses n0's public relays, and n0's DNS to find servers by
node id. If you'd rather not depend on them, run an
[iroh relay](https://github.com/n0-computer/iroh/tree/main/iroh-relay)
and set it on every machine:

```toml
relay_url = "https://relay.example.com"
```

(or `--relay-url` / `CLIPSY_RELAY_URL`). Then nothing goes through n0 at
all.

## Keys

The key lives in `~/.local/state/clipsy/secret.key` (honoring
`XDG_STATE_HOME`). clipsy refuses to use it if other users can access
it. Set `key_file = "/abs/path"` to keep it somewhere else, like a
sops-nix secret.

To rotate it, delete the file, run `clipsy id`, restart `clipsy serve`,
and update the id wherever it's listed.
