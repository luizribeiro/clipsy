# How clipsy compares

clipsy is for getting text to and from the clipboard of the machine you sit
at, from remote machines, including from things that aren't attached to a
terminal. Here's how it stacks up against the usual ways of doing that.

## SSH RemoteForward

A clipboard daemon on the laptop on `127.0.0.1:PORT`, plus
`RemoteForward PORT 127.0.0.1:PORT` in `~/.ssh/config`. Tools on the
remote talk to `localhost:PORT`.

- Only one forward per port per host. The first ssh session binds it; every
  other session to the same host prints
  `Warning: remote port forwarding failed for listen port PORT` and has no
  forward. When the first session closes, the port goes away for all of
  them.
- It only works while an ssh session is open. tmux outlives ssh, so after
  you detach and reattach from a new connection, nvim in the old tmux may
  point at a forward that's gone or owned by another session.
- Anything started outside your ssh connection (cron, agents, CI) can't
  reach it. mosh doesn't forward ports. Hopping from the devbox to another
  box means chaining forwards by hand.
- If you sit at two machines and both forward the same port to one devbox,
  whoever connected first wins.
- Any local user on the remote host can connect to that port while it's
  open and read or write your clipboard. There's no auth.

clipsy doesn't need an ssh session: clients dial the server by key from any
shell, cron job or agent, and the server checks each client's key and its
read/write permissions. With several servers, `CLIPSY_SERVER` can travel
over ssh so you get the clipboard of the machine you came from (see
[More than one server](multiple-servers.md)).

## OSC 52

The terminal sets your local clipboard from escape codes, through ssh and
tmux, with no daemon and no network. If all you need is copying from an
interactive shell, OSC 52 is great and you might not need clipsy.

It does need an attached terminal that supports it, and most terminals turn
off reading the clipboard for safety. So nothing detached (a cron job, an
agent, a tmux session with no client attached) can use it, and
`clipsy read` has no real OSC 52 equivalent.

## lemonade, clipper

Clipboard daemons on a TCP port (clipper can also use a Unix socket).
You expose them with an ssh tunnel or an open port, so they have the
same tunnel problems as above.

## Tailscale plus a small script

`nc` into a `pbcopy` on the laptop over your tailnet. No tunnel, but every
machine has to be on the same tailnet, and anyone on it who can reach the
port gets your clipboard unless you lock it down with ACLs or add auth
yourself.

## KDE Connect, UniClipboard

Desktop apps that sync the clipboard in the background. They're GUI-first,
not something you call from nvim or tmux on a headless box.

## yank

[yank](https://tangled.org/drawbu.dev/yank) is the closest in spirit: a CLI
plus a daemon, peer pairing and Nix modules. It doesn't support macOS.

## dumbpipe

n0's [dumbpipe](https://github.com/n0-computer/dumbpipe) can carry any TCP
tool over iroh, so it removes the tunnel. You'd still run a clipboard daemon
on top, and anyone with the ticket gets in.
