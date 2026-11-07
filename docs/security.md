# Security

Your clipboard has passwords in it, so here's what clipsy does and
doesn't protect:

- Connections are end-to-end encrypted (QUIC with TLS 1.3, keyed to the
  node ids). Relays only see ciphertext, IPs, timing and sizes.
- The server only lets in node ids listed in its config. Everyone else is
  refused, and so is everyone if the config doesn't parse.
- `read` and `write` are separate permissions per client. `read` gives
  that client whatever is on your clipboard, so turn it off for machines
  that only need to send things your way (`read = false`).
- Unlike an SSH reverse forward, the server is reachable whenever it's
  running, not only while you're logged in. But only by machines holding
  a listed key. If one of those keys leaks, it has that client's
  permissions until you remove its entry.
- The server takes at most 8 connections at once, 30 seconds per request
  and 16 MiB per message. Clients give up after 5 seconds if the server
  is asleep or offline, so your editor doesn't hang.
