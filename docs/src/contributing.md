# Contributing

Contributions are welcome via pull requests. Bug reports, feature
requests, and questions are welcome on the
[issue tracker](https://github.com/sidprasad/spytial-rust/issues). The
canonical guide is
[CONTRIBUTING.md](https://github.com/sidprasad/spytial-rust/blob/main/CONTRIBUTING.md);
the notes below cover the common workflows.

## Local setup

```sh
cargo build
cargo test --lib --tests
cargo test --doc
cargo run --example rbt          # add SPYTIAL_NO_OPEN=1 to skip the browser
```

## Before opening a PR

- `cargo fmt` clean, and `cargo clippy --all-targets -- -D warnings` clean.
- Add an entry to the `Unreleased` section of
  [CHANGELOG.md](https://github.com/sidprasad/spytial-rust/blob/main/CHANGELOG.md).
- Changes to decorators need test coverage in `tests/decorators.rs`.
- Keep PRs focused; unrelated cleanups belong in separate PRs.

By contributing you agree your work is dual-licensed under
[MIT](https://github.com/sidprasad/spytial-rust/blob/main/LICENSE-MIT) and
[Apache 2.0](https://github.com/sidprasad/spytial-rust/blob/main/LICENSE-APACHE),
matching the project.

Maintainers: the release process (trusted publishing, crate ordering) is
in [RELEASING.md](https://github.com/sidprasad/spytial-rust/blob/main/RELEASING.md).
