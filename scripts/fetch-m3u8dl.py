#!/usr/bin/env python3
"""Fetch a native N_m3u8DL-RE release and verify the pinned archive and binary."""
import argparse
import hashlib
import io
import json
import os
import pathlib
import tarfile
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOL = "N_m3u8DL-RE"


def download(url):
    request = urllib.request.Request(url, headers={"User-Agent": "yt-dlp-GUI"})
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def binary_from_archive(data, archive, member):
    if archive == "zip":
        with zipfile.ZipFile(io.BytesIO(data)) as source:
            names = [n for n in source.namelist() if pathlib.PurePosixPath(n).name == member]
            if len(names) != 1:
                raise ValueError(f"need exactly one {member} in archive")
            return source.read(names[0])
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as source:
        members = [m for m in source.getmembers() if m.isfile() and pathlib.PurePosixPath(m.name).name == member]
        if len(members) != 1:
            raise ValueError(f"need exactly one {member} in archive")
        return source.extractfile(members[0]).read()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--os", choices=("windows", "linux", "macos"), required=True)
    parser.add_argument("--arch", choices=("x64", "arm64"), default="x64")
    parser.add_argument("--mode", choices=("lock", "latest"), default="lock")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    lock = json.loads((ROOT / "engines.lock.json").read_text(encoding="utf-8"))[TOOL]
    member = TOOL + (".exe" if args.os == "windows" else "")
    target = ROOT / "code" / member
    key = f"{args.os}-{args.arch}"
    pinned = lock[key]
    node = pinned
    if args.mode == "latest":
        release = json.loads(download(f"https://api.github.com/repos/nilaoda/{TOOL}/releases/latest"))
        platform = {"windows": "win", "macos": "osx", "linux": "linux"}[args.os]
        matches = [a for a in release["assets"] if f"_{platform}-{args.arch}_" in a["name"]]
        if len(matches) != 1:
            raise ValueError(f"no unambiguous native release asset for {key}")
        asset = matches[0]
        node = {"url": asset["browser_download_url"], "archive": "zip" if args.os == "windows" else "tar.gz",
                "archiveSha256": (asset.get("digest") or "").removeprefix("sha256:")}
    if args.mode == "lock":
        license_data = (ROOT / lock["licenseFile"]).read_bytes()
        if digest(license_data) != lock["licenseSha256"]:
            raise ValueError("N_m3u8DL-RE license hash mismatch")
        if not args.force and target.is_file() and digest(target.read_bytes()) == node["sha256"]:
            print(f"{TOOL} already matches lock")
            return 0
    print(f"Downloading {node['url']}")
    archive = download(node["url"])
    if not node.get("archiveSha256") or digest(archive) != node["archiveSha256"]:
        raise ValueError("N_m3u8DL-RE archive hash missing or mismatched")
    binary = binary_from_archive(archive, node["archive"], member)
    if args.mode == "lock" and digest(binary) != node["sha256"]:
        raise ValueError("N_m3u8DL-RE binary hash mismatch")
    target.parent.mkdir(parents=True, exist_ok=True)
    staging = target.with_suffix(target.suffix + ".tmp")
    staging.write_bytes(binary)
    staging.chmod(0o755)
    os.replace(staging, target)
    print(f"{TOOL} ready: {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
