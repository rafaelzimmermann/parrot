# Contributing

Use Rust 1.92 or newer and install the native development dependencies listed
in README.md. Before submitting a change, run:

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
bash tests/install.sh.test
```

Installer tests use temporary config files. Never test config mutations against
your live desktop without a backup. Include a regression test for fixes to
config parsing, speech synthesis, or playback sequencing. Report your distro,
compositor version, engine, and exact command when reporting bugs; redact
clipboard text and private paths from logs.
