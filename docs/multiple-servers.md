# More than one server

If you SSH into the devbox from both your `laptop` and your `desktop`, list
both on the devbox and allow the devbox on each of them. The client picks
a server from `--server`, then `CLIPSY_SERVER`, then `default_server`,
and if only one server is listed, that one.

What you usually want is the clipboard of whichever machine you came from.
SSH can carry that for you:

```
# ~/.ssh/config on the laptop and on the desktop
SetEnv CLIPSY_SERVER=laptop     # or desktop

# sshd_config on the devbox
AcceptEnv CLIPSY_SERVER

# tmux.conf on the devbox, so reattaching picks it up
set -ga update-environment CLIPSY_SERVER
```

`--server` also takes a raw node id, or `NAME_OR_ID@IP:PORT` to try a
direct address first (`clipsy serve` prints its ports, but they change on
every restart).
