# Hacking on clipsy

With devenv and direnv:

```bash
direnv allow
cargo build
cargo test
```

The live relay tests need a relay: set `CLIPSY_RELAY_URL` and run
`cargo test --test live -- --ignored`.

The system clipboard test writes to your real clipboard, so it only runs
when asked; on Linux keep it on a throwaway X server:
`CLIPSY_SYSTEM_CLIPBOARD_TESTS=1 xvfb-run -a cargo test --test system_clipboard`.
