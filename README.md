# Rehearsal

**Rehearse your SQLite recovery. Restore, verify, and keep the evidence.**

[日本語](README.ja.md) · [Configuration example](examples/rehearsal.toml) · [Contributing](CONTRIBUTING.md)

Rehearsal is a small Rust CLI for people who run SQLite applications. It copies a
completed backup into a temporary directory, checks the restored database, runs
your application assertions, and saves a JSON report for every completed drill.

One executable. SQLite included. No service or account to set up.

```text
Rehearsal · Notes demo
Backup    rehearsal-demo/app.backup.sqlite

  PASS  Restore
  PASS  Backup age
  PASS  Open database
  PASS  SQLite integrity
  PASS  Foreign keys
  PASS  SQL: Users exist
  PASS  SQL: At least two notes survived
  PASS  SQL: Recent note exists
  PASS  Cleanup
```

Abbreviated output; actual runs also show details, durations, and the report path.

## Try it

Download an archive from [GitHub Releases](https://github.com/nktkt/rehearsal/releases/latest).
These executables include SQLite and run without installing Rust.

| Computer | Choose the archive ending in |
| --- | --- |
| Linux x64 | `x86_64-unknown-linux-musl.tar.gz` |
| Mac, Apple Silicon (macOS 11+) | `aarch64-apple-darwin.tar.gz` |
| Mac, Intel (macOS 11+) | `x86_64-apple-darwin.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc.zip` |

Extract it into a new directory, open a terminal there, and run:

```sh
./rehearsal demo
./rehearsal history --config rehearsal-demo/rehearsal.toml
```

On Windows PowerShell, use `.\rehearsal.exe` in place of `./rehearsal`.
You can move the executable into a directory on your `PATH` to use `rehearsal`
from anywhere. Each archive has a matching SHA-256 checksum; see
[download verification](RELEASING.md#verify-a-download). Mac and Windows binaries
are unsigned, and Apple notarization is not included.

## Build from source

Install a current stable [Rust toolchain](https://rustup.rs/), then run these
commands from this checkout. Building bundled SQLite also requires a C compiler
(Xcode Command Line Tools on macOS; a build toolchain such as `build-essential`
on Ubuntu).

```sh
cargo run --locked -- demo
cargo run --locked -- history --config rehearsal-demo/rehearsal.toml
```

The demo creates `rehearsal-demo/` with a sample database, configuration, and a
report. To run it again:

```sh
cargo run --locked -- check --config rehearsal-demo/rehearsal.toml
```

See a real failure with an intentionally orphaned foreign key:

```sh
cargo run --locked -- demo --broken --dir rehearsal-broken-demo
# Exits 1, and saves a failed report.
```

Install the executable from this checkout:

```sh
cargo install --locked --path .
rehearsal --help
```

This project has not been published to crates.io. Use a GitHub release or build
from this checkout.

## Check your backup

Start with a **completed, standalone, uncompressed SQLite backup file**:

```sh
rehearsal check /backups/app.sqlite
rehearsal check /backups/app.sqlite --max-age 24h --json
```

For repeatable drills, generate a configuration:

```sh
rehearsal init --backup /backups/app.sqlite
rehearsal check
rehearsal history
```

`init` refuses to overwrite an existing configuration and records the backup as
an absolute path. Direct file checks use `.rehearsal/reports` in the current
directory. Each completed run, including failures, creates a separate report.

## Configuration

```toml
name = "Notes production backup"
backup = "backups/app.sqlite"
report_dir = ".rehearsal/reports"
max_age = "24h"
timeout = "30s"

[[checks]]
name = "Users survived"
sql = "SELECT EXISTS(SELECT 1 FROM users)"

[[checks]]
name = "Recent note exists"
sql = "SELECT COALESCE(MAX(created_at) >= datetime('now', '-1 day'), 0) FROM notes"
```

- Paths inside a config are relative to **that config file**, regardless of the
  process's current directory. CLI paths are relative to the current directory.
- `name` defaults to `SQLite backup`; `report_dir` defaults to `.rehearsal/reports`.
- `max_age` is optional. It checks the source file's modification time, and rejects
  timestamps in the future. It does **not** measure the age of data in the database.
  Copying a backup can change its timestamp. Use an application SQL assertion to
  test data freshness. The example assumes UTC timestamps in SQLite's
  `YYYY-MM-DD HH:MM:SS` format.
- `timeout` defaults to `30s` **per SQLite query**, including the integrity and
  foreign key checks. SQLite's progress callback interrupts long queries. This is
  a cooperative query timeout, not a wall-clock deadline for copying files or
  blocked filesystem I/O.
- A SQL assertion must be one read-only query returning exactly **one row, one
  column, integer `1`**. False, NULL, strings, other numbers, missing tables, extra
  rows, extra statements, and SQL errors fail the check. `SELECT 1` by itself
  provides no application-specific assurance.
- Writes, `ATTACH`, and user-supplied PRAGMAs are rejected. Loadable extensions are
  disabled. Application-specific extensions and custom collations are not supported.
- Unknown configuration fields and duplicate assertion names are errors.
- `--report-dir`, `--max-age`, and `--timeout` override valid configuration values.

## What a drill verifies

1. Check the SQLite header and copy the backup into a private temporary directory.
2. Check its file age, if requested.
3. Open the restored copy read-only.
4. Run a full `PRAGMA integrity_check`.
5. Run `PRAGMA foreign_key_check` separately.
6. Run each configured SQL assertion.
7. Remove the temporary copy and atomically save a report.

An integrity failure skips foreign key and application checks. An individual
assertion failure does not prevent later assertions from running. Reports contain
the status and duration of each step, source path, backup size and timestamp,
tool/SQLite versions, and failure details. They do not contain database row dumps.

The original backup is opened only as a read-only file; SQLite operates on the
temporary copy. Reports never overwrite existing files. Keep enough free space
in your OS temporary directory for one restored database. Report history is kept
until you remove it; v0.1 has no automatic retention policy.

SQLite's integrity check does not check foreign key constraints, which is why
Rehearsal performs both checks. See [SQLite's PRAGMA documentation](https://www.sqlite.org/pragma.html#pragma_integrity_check).

## Preparing a valid input

Make snapshots with SQLite's online backup API, the SQLite CLI's `.backup`
command, or `VACUUM INTO`. For example, if the SQLite CLI is installed:

```sh
sqlite3 /srv/app/app.sqlite ".backup '/backups/app.sqlite'"
rehearsal check /backups/app.sqlite
```

Publish the backup only after the backup command has finished. Do not give
Rehearsal a live database or a raw file copy taken during writes. It rejects inputs
with `-wal`, `-shm`, or `-journal` siblings and detects file size/mtime changes
during the copy. These checks cannot prove a file was created as a consistent
snapshot; creating a completed backup remains the producer's responsibility.
See [SQLite's backup guidance](https://www.sqlite.org/howtocorrupt.html#_backup_or_restore_while_a_transaction_is_active).

Rehearsal v0.1 accepts local files. For a Litestream replica, first restore it to
a new staging path with Litestream, then check that file:

```sh
litestream restore -o /backups/drill.sqlite s3://my-backups/app.sqlite &&
  rehearsal check /backups/drill.sqlite
```

Choose a new staging path for each restore, or use an existing backup job that
publishes a completed snapshot. The `&&` prevents checking an older file after a
failed restore. Object storage credentials and remote restores are managed by
Litestream, not Rehearsal. See [Litestream's restore command](https://litestream.io/reference/restore/).

## Automation and exit codes

| Exit | Meaning |
| --- | --- |
| `0` | All required checks passed and the report was saved. |
| `1` | A drill failed, including missing/corrupt backups, age, SQL, or cleanup failures. The report was saved. |
| `2` | Usage, configuration, setup, output, or report persistence error. |

`check --json` writes exactly one report to stdout for a completed drill, with
diagnostics on stderr. Configuration/setup failures produce no JSON report.
If report persistence fails, the completed report still goes to stdout and the
process exits `2`; its `status` describes verification only. Always inspect the
exit code as well as the JSON when automating.

```sh
rehearsal check --config /srv/app/rehearsal.toml --json
rehearsal history --config /srv/app/rehearsal.toml --limit 5 --json
```

Run `rehearsal check --config /absolute/path/rehearsal.toml` from cron, a systemd
timer, or your existing scheduler. Configure that scheduler to alert on nonzero
exit codes. Rehearsal itself does not send notifications or install scheduled jobs.

## Scope

A passing drill establishes that this particular restored SQLite file passes the
configured checks with the bundled SQLite version. It does not test application
startup, uploaded files, encryption keys, external services, or disaster recovery
of an entire server. The initial release supports plain SQLite snapshots, not
compressed archives, SQL dumps, SQLCipher databases, or direct object storage URLs.

Future work can build on real usage: restore adapters, application startup probes,
notifications, and report retention. Contributions should keep the local workflow
small and understandable.

## Development

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
```

The CI workflow is configured for Linux, macOS, and Windows. `Cargo.lock` is
included for reproducible application builds. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE).
