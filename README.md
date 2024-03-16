# 📋 clipsy

Sync your clipboard across hosts.

## Usage

Start the server on your laptop:

```bash
clipsy serve
```

Then setup your SSH to forward port 52697 by adding this to your
`~/.ssh/config`:

```
Host *
  RemoteForward 52697 127.0.0.1:52697
```

Then, from the remote host you can write to your clipboard with:

```
clipsy write foobar
```

Alternatively, you can also stream to your clipboard:

```
cat somefile.txt | clipsy write
```

## Integrations

<details>
<summary><b>neovim</b></summary>

With [vim-plug](https://github.com/junegunn/vim-plug), simply add to your config:

```
Plug 'luizribeiro/clipsy'
```

And anything copied through the registers `+` and `*` will be automatically synced
to your clipsy server.
</details>

<details>
<summary><b>tmux</b></summary>

With [tpm](https://github.com/tmux-plugins/tpm) (Tmux Plugin Manager),
simply add to your config:

```
set -g @plugin 'luizribeiro/clipsy'
```

And anything copied through tmux will be automatically synced to your
clipsy server.
</details>

## Developing

We use [devenv](https://devenv.sh) and [direnv](https://direnv.net) to
build, so simply run:

```
direnv allow
cargo build
```
