# Contributing to Rehearsal

Rehearsal should make one job easy: verifying that a completed SQLite backup can
be restored and that its contents meet the application's expectations.

## Local development

Use the current stable Rust toolchain and a C compiler for bundled SQLite.

```sh
cargo run --locked -- demo
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests create their own SQLite databases in temporary directories. They require no
credentials, remote services, or external SQLite command. Use `demo --dir PATH`
with a fresh directory to inspect output while developing.

## Changes

- Explain the user problem and the behavior after your change.
- Keep defaults useful and failure messages actionable.
- Preserve the original backup and never silently replace user files.
- Add regression coverage when a change affects recovery correctness, exit codes,
  SQL assertions, or report persistence.
- Treat JSON reports as an interface. Version incompatible schema changes.
- Update the English and Japanese documentation when CLI behavior changes.
- Keep `Cargo.lock` in version control. Explain dependency additions.

The first release deliberately focuses on local, completed SQLite snapshots.
Restore adapters should reuse existing backup systems and clearly distinguish
restore failure, verification failure, and operational failure.

## Before publishing to crates.io

The source repository is [nktkt/rehearsal](https://github.com/nktkt/rehearsal).
The executable and package currently use the name `rehearsal`. Confirm registry
name availability before the first crates.io release. There is no automated
publishing workflow.

Run `cargo package --locked --allow-dirty` to verify a source package locally;
review its contents with `cargo package --list`. Publishing is a separate action.
