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

## Installation

<details>
<summary><b>Building from Source</b></summary>

You can install with Rust's `cargo`:

```
cargo install --git https://github.com/luizribeiro/clipsy
```

</details>

<details>
<summary><b>NixOS</b></summary>

On your `flake.nix`, add `clipsy` as an `input`

```nix
{
  inputs.sops-nix.url = "github:luizribeiro/clipsy;

  outputs = { self, nixpkgs, clipsy }: {
    # change `yourhostname` to your actual hostname
    nixosConfigurations.yourhostname = nixpkgs.lib.nixosSystem {
      # customize to your system
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        clipsy.nixosModules.linux
      ];
    };
  };
}
```

With the module loaded, you can enable `clipsy` as a service:

```nix
{ ... }:

{
  services.clipsy.enable = true;
}
```

If you setup the overlay `clipsy.overlays.default`, you will be able
to install `clipsy` to your `PATH` as well (which is necessary for the
neovim and tmux integrations):

```nix
{ pkgs, clipsy, ... }:

{
  nixpkgs.overlays = [
    clipsy.overlays.default
  ];
  environment.systemPackages = with pkgs; [ clipsy ];
}
```

</details>

<details>
<summary><b>nix-darwin</b></summary>

The instructions are the same as NixOS', with the difference that the
module is `clipsy.nixosModules.darwin` instead of
`clipsy.nixosModules.linux`.

</details>

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

## TODOs

* Fix crash when clipboard contains image
