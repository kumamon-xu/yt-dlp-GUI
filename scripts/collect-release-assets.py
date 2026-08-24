#!/usr/bin/env python3
"""Collect and checksum the installer files produced by one Tauri matrix job."""

import argparse
import hashlib
import json
import pathlib
import shutil
import sys


SUFFIXES = {
    "windows-x64": (".msi", ".exe"),
    "linux-x64": (".deb", ".appimage"),
    "linux-arm64": (".deb", ".appimage"),
    "macos-arm64": (".dmg",),
    "macos-x64": (".dmg",),
}


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bundle-root", required=True)
    parser.add_argument("--platform", choices=sorted(SUFFIXES), required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    bundle_root = pathlib.Path(args.bundle_root)
    out = pathlib.Path(args.out)
    if not bundle_root.is_dir():
        sys.exit(f"bundle directory not found: {bundle_root}")
    out.mkdir(parents=True, exist_ok=True)
    if any(out.iterdir()):
        sys.exit(f"release output directory must be empty: {out}")

    suffixes = SUFFIXES[args.platform]
    candidates = sorted(
        path
        for path in bundle_root.rglob("*")
        if path.is_file()
        and path.suffix.lower() in suffixes
        and args.version in path.name
    )
    if not candidates:
        sys.exit(f"no {args.version} installer for {args.platform} under {bundle_root}")

    files = []
    names = set()
    for source in candidates:
        if source.name in names:
            sys.exit(f"duplicate installer basename: {source.name}")
        names.add(source.name)
        destination = out / source.name
        shutil.copy2(source, destination)
        if destination.stat().st_size == 0:
            sys.exit(f"empty installer: {destination}")
        files.append(
            {
                "name": destination.name,
                "bytes": destination.stat().st_size,
                "sha256": sha256(destination),
            }
        )

    manifest = {"schema": 1, "platform": args.platform, "files": files}
    (out / "asset-manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    print(f"collected {len(files)} installer(s) for {args.platform}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
