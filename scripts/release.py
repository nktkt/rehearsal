#!/usr/bin/env python3
"""Package, exercise, and verify Rehearsal binaries using Python's standard library."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT / "dist"
TARGETS = (
    "x86_64-unknown-linux-musl",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
)


def version():
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        return tomllib.load(manifest)["package"]["version"]


def archive_name(target):
    extension = ".zip" if "windows" in target else ".tar.gz"
    return f"rehearsal-v{version()}-{target}{extension}"


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def invoke(binary, directory, arguments, expected_code=0):
    result = subprocess.run(
        [str(binary), *arguments], cwd=directory, capture_output=True,
        text=True, encoding="utf-8", timeout=30,
    )
    if result.returncode != expected_code:
        raise RuntimeError(
            f"{arguments}: expected exit {expected_code}, got {result.returncode}\n"
            f"{result.stdout}\n{result.stderr}"
        )
    return result.stdout


def smoke_test(archive, target):
    # Exercise the actual bytes being distributed, including archive permissions.
    with tempfile.TemporaryDirectory(prefix="rehearsal-package-") as directory:
        directory = Path(directory)
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as bundle:
                bundle.extractall(directory)
        else:
            with tarfile.open(archive) as bundle:
                bundle.extractall(directory, filter="data")
        binary = directory / ("rehearsal.exe" if "windows" in target else "rehearsal")
        actual_version = invoke(binary, directory, ["--version"]).strip()
        if actual_version != f"rehearsal {version()}":
            raise RuntimeError(f"unexpected executable version: {actual_version}")
        invoke(binary, directory, ["demo"])
        report = json.loads(invoke(binary, directory, [
            "check", "--config", "rehearsal-demo/rehearsal.toml", "--json",
        ]))
        if report["status"] != "passed" or report["tool_version"] != version():
            raise RuntimeError("packaged executable did not produce a passing report")
        invoke(binary, directory, ["demo", "--broken", "--dir", "broken-demo"], 1)
        history = json.loads(invoke(binary, directory, [
            "history", "--config", "rehearsal-demo/rehearsal.toml", "--json",
        ]))
        if len(history) != 2 or any(run["status"] != "passed" for run in history):
            raise RuntimeError("packaged executable did not preserve report history")


def package(target):
    executable = "rehearsal.exe" if "windows" in target else "rehearsal"
    binary = ROOT / "target" / target / "release" / executable
    if not binary.is_file():
        raise RuntimeError(f"build the target first: {binary}")
    files = [(binary, executable)]
    files.extend((ROOT / name, name) for name in (
        "LICENSE", "README.md", "README.ja.md", "CONTRIBUTING.md", "RELEASING.md",
        "examples/rehearsal.toml",
    ))
    DIST.mkdir(exist_ok=True)
    archive = DIST / archive_name(target)
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            for source, name in files:
                bundle.write(source, name)
    else:
        with tarfile.open(archive, "w:gz") as bundle:
            for source, name in files:
                bundle.add(source, arcname=name)
    smoke_test(archive, target)
    checksum = archive.with_name(archive.name + ".sha256")
    checksum.write_text(f"{digest(archive)}  {archive.name}\n", encoding="utf-8")
    print(f"Packaged and tested: {archive.name} ({archive.stat().st_size:,} bytes)")


def verify_dist():
    expected = set()
    for target in TARGETS:
        archive = DIST / archive_name(target)
        checksum = archive.with_name(archive.name + ".sha256")
        expected.update((archive.name, checksum.name))
        actual = checksum.read_text(encoding="utf-8")
        if actual != f"{digest(archive)}  {archive.name}\n":
            raise RuntimeError(f"checksum mismatch: {archive.name}")
    actual_files = {path.name for path in DIST.iterdir()}
    if actual_files != expected:
        raise RuntimeError(f"unexpected or missing release files: {actual_files ^ expected}")
    print(f"Verified {len(TARGETS)} archives and their SHA-256 checksums.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("check-version")
    check.add_argument("tag", nargs="?", default="")
    pack = commands.add_parser("package")
    pack.add_argument("--target", choices=TARGETS, required=True)
    commands.add_parser("verify-dist")
    args = parser.parse_args()
    if args.command == "check-version":
        tag = f"v{version()}"
        if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
            parser.error("release versions must be stable versions such as 0.1.0")
        if args.tag and args.tag != tag:
            parser.error(f"tag {args.tag!r} does not match Cargo.toml ({tag})")
        print(f"Version verified: {tag}")
    elif args.command == "package":
        package(args.target)
    else:
        verify_dist()


if __name__ == "__main__":
    main()
