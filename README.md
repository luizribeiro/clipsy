# 📋 clipsy

clipsy helps you keep your clipboard in sync across your machines with
[iroh](https://iroh.computer), on Linux or macOS. Copy
something in nvim or tmux on a machine you SSH into and it lands on the
clipboard of the machine you're sitting at.

Some people share their clipboard over an SSH tunnel, but that only
works in the first ssh session to a host, and only while that session is
open, so cron jobs and agents can't use it. With iroh, machines dial each
other by public key, so there are no ports to open and no tunnels.

If you only copy from an interactive shell, OSC 52 might be all you need.
See [how clipsy compares](docs/alternatives.md).

## Install

```bash
cargo install --git https://github.com/luizribeiro/clipsy
```

Or with Nix, see [docs/nix.md](docs/nix.md).

## Quick start

Say you're on your `laptop` and you ssh into a `devbox`. The laptop has
the clipboard you care about, so it runs the server.

Get the node id of each machine:

```bash
clipsy id
```

On the laptop, in `~/.config/clipsy/config.toml`:

```toml
[clients.devbox]
id = "<devbox's node id>"
read = true
write = true
```

On the devbox:

```toml
default_server = "laptop"

[servers]
laptop = "<laptop's node id>"
```

Run `clipsy serve` on the laptop, and then on the devbox:

```bash
clipsy write foobar
cat somefile.txt | clipsy write
clipsy read
```

Node ids are public keys, so you can keep these configs in your dotfiles.

## nvim and tmux

```vim
" vim-plug
Plug 'luizribeiro/clipsy'
```

```
# tpm
set -g @plugin 'luizribeiro/clipsy'
```

With these, yanking in nvim (the `+` and `*` registers) and copying in
tmux go to the server. They need `clipsy` on your `PATH`.

## Docs

- [More than one server](docs/multiple-servers.md)
- [Config](docs/config.md)
- [Security](docs/security.md)
- [Nix](docs/nix.md)
- [How clipsy compares](docs/alternatives.md)
- [Hacking on it](docs/hacking.md)

## License

MIT, see [LICENSE.md](LICENSE.md).
