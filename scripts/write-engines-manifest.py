#!/usr/bin/env python3
"""Record bundled engine hashes, versions, and source URLs for a Release asset."""
import argparse, hashlib, json, os, pathlib, subprocess, sys
from typing import Optional

def sha256(p: pathlib.Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

def first_line(cmd) -> str:
    try:
        out = subprocess.check_output(cmd, stderr=subprocess.STDOUT, timeout=30)
        return out.decode("utf-8", "replace").splitlines()[0].strip()
    except Exception as e:
        return f"error: {e}"

def yt_key(os_name: str, arch: str) -> str:
    os_name = os_name.lower()
    if os_name.startswith("win"):
        return "windows-x64"
    if os_name.startswith("darwin") or os_name.startswith("mac"):
        return "macos"
    if arch == "arm64":
        return "linux-arm64"
    return "linux-x64"

def ff_key(os_name: str, arch: str) -> str:
    os_name = os_name.lower()
    if os_name.startswith("win"):
        return "windows-x64"
    if os_name.startswith("darwin") or os_name.startswith("mac"):
        return f"macos-{arch}"
    return f"linux-{arch}"

def lock_url(lock: dict, tool: str, key: str) -> Optional[str]:
    node = (lock.get(tool) or {}).get(key) or {}
    return node.get("url")

def latest_url(os_name: str, arch: str, tool: str) -> Optional[str]:
    os_name = os_name.lower()
    if tool == "yt-dlp":
        if os_name.startswith("win"):
            return "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
        if os_name.startswith("darwin") or os_name.startswith("mac"):
            return "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
        if arch == "arm64":
            return "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64"
        return "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
    if os_name.startswith("win"):
        return "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip"
    plat = {"linux": "linux", "darwin": "darwin", "macos": "darwin"}.get(os_name, os_name)
    if plat == "linux" or os_name.startswith("linux"):
        return f"https://github.com/eugeneware/ffmpeg-static/releases/latest/download/ffmpeg-linux-{arch}"
    return f"https://github.com/eugeneware/ffmpeg-static/releases/latest/download/ffmpeg-darwin-{arch}"

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--os", default=os.environ.get("RUNNER_OS", ""))
    ap.add_argument("--arch", default="x64")
    ap.add_argument("--mode", choices=("lock", "latest"), default="lock")
    ap.add_argument("--label", default="")
    args = ap.parse_args()
    root = pathlib.Path(__file__).resolve().parents[1]
    lock = json.loads((root / "engines.lock.json").read_text(encoding="utf-8"))
    code = root / "code"
    yk, fk = yt_key(args.os, args.arch), ff_key(args.os, args.arch)
    engines = []
    for n, tool, key in (("yt-dlp.exe", "yt-dlp", yk), ("yt-dlp", "yt-dlp", yk), ("ffmpeg.exe", "ffmpeg", fk), ("ffmpeg", "ffmpeg", fk)):
        p = code / n
        if not (p.is_file() and p.stat().st_size > 0):
            continue
        lock_u = lock_url(lock, tool, key)
        lat_u = latest_url(args.os, args.arch, tool)
        entry = {
            "name": n,
            "tool": tool,
            "lock_key": key,
            "path": str(p.relative_to(root)).replace("\\", "/"),
            "sha256": sha256(p),
            "bytes": p.stat().st_size,
            "lock_url": lock_u,
            "latest_url": lat_u,
            "url": lock_u if args.mode == "lock" else lat_u,
            "fetch_mode": args.mode,
            "version": first_line([str(p), "-version" if "ffmpeg" in n.lower() else "--version"]),
        }
        engines.append(entry)
    if not engines:
        print("no engine binaries under code/", file=sys.stderr)
        return 1
    doc = {
        "ref": os.environ.get("GITHUB_REF", ""),
        "sha": os.environ.get("GITHUB_SHA", ""),
        "label": args.label,
        "os": args.os,
        "arch": args.arch,
        "fetch_mode": args.mode,
        "engines": engines,
    }
    out = pathlib.Path(args.out)
    out.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {out} ({len(engines)} files)")
    return 0

if __name__ == "__main__":
    sys.exit(main())
