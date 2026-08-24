#!/usr/bin/env python3
"""Validate all matrix artifacts and prepare one flat, publishable asset set."""

import argparse
import hashlib
import json
import pathlib
import shutil
import sys


PLATFORMS = (
    "windows-x64",
    "linux-x64",
    "linux-arm64",
    "macos-arm64",
    "macos-x64",
)
PLATFORM_INFO = {
    "windows-x64": ("windows", "x64", (".msi", ".exe")),
    "linux-x64": ("linux", "x64", (".deb", ".appimage")),
    "linux-arm64": ("linux", "arm64", (".deb", ".appimage")),
    "macos-arm64": ("macos", "arm64", (".dmg",)),
    "macos-x64": ("macos", "x64", (".dmg",)),
}
LEGAL_FILES = (
    "THIRD_PARTY_NOTICES.md",
    "licenses/FFmpeg-GPL-3.0.txt",
    "licenses/yt-dlp-Unlicense.txt",
)


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def copy_unique(source: pathlib.Path, out: pathlib.Path, names: set[str]) -> pathlib.Path:
    if source.name in names:
        sys.exit(f"duplicate release asset basename: {source.name}")
    names.add(source.name)
    destination = out / source.name
    shutil.copy2(source, destination)
    return destination


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifacts-root", required=True)
    parser.add_argument("--project-root", default=".")
    parser.add_argument("--out", required=True)
    parser.add_argument("--fetch-mode", choices=("lock", "latest"), required=True)
    args = parser.parse_args()

    artifacts_root = pathlib.Path(args.artifacts_root)
    project_root = pathlib.Path(args.project_root)
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    if any(out.iterdir()):
        sys.exit(f"publish output directory must be empty: {out}")
    names: set[str] = set()
    published: list[pathlib.Path] = []
    lock = json.loads((project_root / "engines.lock.json").read_text(encoding="utf-8"))

    for platform in PLATFORMS:
        expected_os, expected_arch, suffixes = PLATFORM_INFO[platform]
        artifact = artifacts_root / f"release-{platform}"
        manifest_path = artifact / "asset-manifest.json"
        if not manifest_path.is_file():
            sys.exit(f"missing artifact manifest: {manifest_path}")
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if manifest.get("schema") != 1 or manifest.get("platform") != platform:
            sys.exit(f"invalid artifact manifest: {manifest_path}")
        files = manifest.get("files") or []
        if not files:
            sys.exit(f"artifact contains no installers: {artifact}")
        for expected in files:
            source = artifact / expected["name"]
            if source.suffix.lower() not in suffixes:
                sys.exit(f"unexpected installer type for {platform}: {source.name}")
            if not source.is_file():
                sys.exit(f"missing installer: {source}")
            if source.stat().st_size != expected["bytes"]:
                sys.exit(f"size mismatch: {source}")
            if sha256(source) != expected["sha256"]:
                sys.exit(f"checksum mismatch: {source}")
            published.append(copy_unique(source, out, names))

        engine_manifests = list(artifact.glob(f"engines-manifest-{platform}.json"))
        if len(engine_manifests) != 1:
            sys.exit(f"need one engine manifest for {platform}")
        engine_doc = json.loads(engine_manifests[0].read_text(encoding="utf-8"))
        actual_os = str(engine_doc.get("os", "")).lower()
        if not actual_os.startswith(expected_os) or engine_doc.get("arch") != expected_arch:
            sys.exit(f"engine manifest platform mismatch: {engine_manifests[0]}")
        if engine_doc.get("fetch_mode") != args.fetch_mode:
            sys.exit(f"engine manifest fetch mode mismatch: {engine_manifests[0]}")
        engines = engine_doc.get("engines") or []
        if {entry.get("tool") for entry in engines} != {"yt-dlp", "ffmpeg"}:
            sys.exit(f"engine manifest needs yt-dlp and ffmpeg: {engine_manifests[0]}")
        for entry in engines:
            digest = str(entry.get("sha256", ""))
            if len(digest) != 64 or entry.get("bytes", 0) < 1_000_000:
                sys.exit(f"invalid engine hash/size: {engine_manifests[0]}")
            if not entry.get("license") or not entry.get("license_file") or not entry.get("source_url"):
                sys.exit(f"incomplete engine compliance metadata: {engine_manifests[0]}")
            if args.fetch_mode == "lock" and digest != entry.get("locked_sha256"):
                sys.exit(f"locked engine checksum mismatch: {engine_manifests[0]}")
        published.append(copy_unique(engine_manifests[0], out, names))

    for relative in LEGAL_FILES:
        source = project_root / relative
        if not source.is_file() or source.stat().st_size == 0:
            sys.exit(f"missing legal file: {source}")
        if relative == "licenses/FFmpeg-GPL-3.0.txt":
            expected_hash = lock["ffmpeg"]["licenseSha256"]
            if sha256(source) != expected_hash:
                sys.exit(f"FFmpeg license checksum mismatch: {source}")
        if relative == "licenses/yt-dlp-Unlicense.txt":
            expected_hash = lock["yt-dlp"]["licenseSha256"]
            if sha256(source) != expected_hash:
                sys.exit(f"yt-dlp license checksum mismatch: {source}")
        published.append(copy_unique(source, out, names))

    checksum_file = out / "SHA256SUMS.txt"
    checksum_file.write_text(
        "".join(f"{sha256(path)}  {path.name}\n" for path in sorted(published)),
        encoding="utf-8",
    )
    print(f"validated five platforms; prepared {len(published) + 1} release assets")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
