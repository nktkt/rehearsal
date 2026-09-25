# Releasing Rehearsal

The [Release workflow](.github/workflows/release.yml) builds four native targets:
Linux x64 (musl), Apple Silicon Mac, Intel Mac, and Windows x64. SQLite is bundled.
Mac builds target macOS 11 or newer. Binaries are not developer-signed or notarized.

Each build runs formatting, Clippy, and the Rust tests. It then creates an archive,
extracts it, and checks the executable version, a successful drill, an intentionally
failed drill, JSON output, and history. Only then is an artifact uploaded.

## Preview without publishing

Push the workflow to the default branch, then run:

```sh
gh workflow run release.yml --ref main
gh run list --workflow release.yml
```

A manual run builds and tests all four archives. It never publishes a release,
even if dispatched against a tag. Archives and checksums remain available as
Actions artifacts for seven days.

The package script uses Python 3.12+ and its standard library:

```sh
cargo build --locked --release --target aarch64-apple-darwin
python3 scripts/release.py check-version
python3 scripts/release.py package --target aarch64-apple-darwin
```

Run packaging on a machine that can execute the target binary. Outputs go to
`dist/`, which is ignored by Git.

## Publish a version

1. Update `version` in `Cargo.toml` and update `Cargo.lock` with Cargo.
2. Update documentation and `.github/release-notes.md` for the release.
3. Push the commit to `main` and confirm CI succeeds.
4. Create and push an annotated tag matching the manifest version:

```sh
git tag -a v0.1.0 -m "Release Rehearsal v0.1.0"
git push origin v0.1.0
```

Tag pushes trigger a fresh build of the exact tagged commit. A mismatched tag
fails before packaging. All four builds must succeed before the publication job
can run. The publication job verifies every checksum, creates a draft release,
uploads all eight files, and finally publishes the release.

Only the publication job has permission to write repository contents. It uses
GitHub's automatically issued workflow token; no additional secret is required.

If a build fails, fix the cause and rerun as appropriate. A partially uploaded
draft can be resumed by rerunning the failed workflow. An already published
release is never overwritten by the workflow. For source fixes after publication,
increment the version and publish a new tag instead of moving an existing tag.

## Verify a download

Download the archive and its matching `.sha256` file into the same directory:

```sh
# macOS example
shasum -a 256 -c rehearsal-v0.1.0-aarch64-apple-darwin.tar.gz.sha256

# Linux example
sha256sum -c rehearsal-v0.1.0-x86_64-unknown-linux-musl.tar.gz.sha256
```

On Windows, compare `Get-FileHash -Algorithm SHA256 ARCHIVE.zip` with the digest
in `ARCHIVE.zip.sha256`. Checksums detect damaged downloads; they are not a code
signing certificate.
