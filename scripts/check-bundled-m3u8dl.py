#!/usr/bin/env python3
"""Verify and execute the exact RE payload bundled into Linux installers."""
import argparse
import gzip
import hashlib
import os
import pathlib
import subprocess
import tempfile


def unpack(archive):
    expected = archive.with_suffix(".sha256").read_text(encoding="ascii").strip().lower()
    binary = gzip.decompress(archive.read_bytes())
    if len(expected) != 64 or hashlib.sha256(binary).hexdigest() != expected:
        raise ValueError(f"RE payload checksum mismatch: {archive}")
    return binary


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    args = parser.parse_args()
    archives = [p for p in pathlib.Path(args.root).rglob("N_m3u8DL-RE.gz") if p.parent.name == "code"]
    if not archives:
        raise ValueError("bundled Linux RE payload missing")
    for archive in archives:
        binary = unpack(archive)
        with tempfile.TemporaryDirectory(prefix="re-bundle-smoke-") as directory:
            executable = pathlib.Path(directory) / "N_m3u8DL-RE"
            executable.write_bytes(binary)
            executable.chmod(0o755)
            out = subprocess.run([str(executable), "--version"], capture_output=True, check=True, timeout=30,
                                 env={**os.environ, "TERM": "dumb"})
            print(f"ok {archive}: verified {len(binary)} bytes; {out.stdout.decode('utf-8', 'replace').strip()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
