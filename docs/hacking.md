# Hacking on clipsy

With devenv and direnv:

```bash
direnv allow
cargo build
cargo test
```

The live relay tests need a relay: set `CLIPSY_RELAY_URL` and run
`cargo test --test live -- --ignored`.
