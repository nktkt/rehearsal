Rehearsal restores a completed SQLite backup into a temporary directory, verifies
its integrity and foreign keys, runs your SQL assertions, and saves a JSON report.

Download the archive for your computer. Rust and a separate SQLite installation
are not required to run these executables.

| Computer | Archive suffix |
| --- | --- |
| Linux x64 | `x86_64-unknown-linux-musl.tar.gz` |
| Mac, Apple Silicon (macOS 11+) | `aarch64-apple-darwin.tar.gz` |
| Mac, Intel (macOS 11+) | `x86_64-apple-darwin.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc.zip` |

Each archive includes the executable, MIT license, English/Japanese guides, and a
sample configuration. A matching `.sha256` file lets you verify the download.
The Linux executable uses musl; the Mac and Windows binaries are unsigned.
Apple notarization is not included.

Extract into a new directory and run `./rehearsal demo` (PowerShell:
`.\rehearsal.exe demo`). A sample backup, configuration, and verification report
will be created in `rehearsal-demo/`.

[English guide](https://github.com/nktkt/rehearsal#readme) ·
[日本語ガイド](https://github.com/nktkt/rehearsal/blob/main/README.ja.md)

This release accepts local, completed SQLite snapshots. A passing drill verifies
the configured database checks; application startup and full-server recovery are
outside its scope. crates.io publication is separate from this binary release.
