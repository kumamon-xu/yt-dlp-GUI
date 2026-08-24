#!/usr/bin/env bash
# Download yt-dlp + ffmpeg into code/.
#   --lock   use engines.lock.json (Stable)
#   --latest ignore lock (Nightly)
#   --arch auto|x64|arm64
set -euo pipefail

FORCE=0
ARCH=auto
MODE=lock
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCK="$ROOT/engines.lock.json"
CODE="$ROOT/code"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --force|-f) FORCE=1; shift ;;
    --lock) MODE=lock; shift ;;
    --latest) MODE=latest; shift ;;
    --arch) ARCH="${2:?}"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

mkdir -p "$CODE"
OS_RAW="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$OS_RAW" in
  darwin) OS=macos ;;
  linux) OS=linux ;;
  *) echo "unsupported OS: $OS_RAW" >&2; exit 1 ;;
esac
if [[ "$ARCH" == auto ]]; then
  case "$(uname -m)" in
    x86_64|amd64) ARCH=x64 ;;
    aarch64|arm64) ARCH=arm64 ;;
    *) echo "unsupported arch" >&2; exit 1 ;;
  esac
fi

sha256_of() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

lock_get() {
  python3 - "$LOCK" "$1" "$2" <<'PY'
import json,sys
lock=json.load(open(sys.argv[1]))
tool, key=sys.argv[2], sys.argv[3]
node=lock[tool][key]
print(node["url"])
print(node["sha256"])
print(node.get("archive") or "")
print(node.get("member") or "")
PY
}

YTDLP="$CODE/yt-dlp"
FFMPEG="$CODE/ffmpeg"

yt_key="$OS-$ARCH"
[[ "$OS" == macos ]] && yt_key=macos
ff_key="$OS-$ARCH"
[[ "$OS" == macos ]] && ff_key="macos-$ARCH"

download() {
  echo "Downloading $1"
  curl -L --fail --retry 3 --retry-delay 2 --connect-timeout 30 -o "$2" "$1"
}

if [[ "$MODE" == lock ]]; then
  mapfile -t YT < <(lock_get yt-dlp "$yt_key")
  download "${YT[0]}" "$YTDLP"
  chmod +x "$YTDLP"
  got=$(sha256_of "$YTDLP")
  exp=$(echo "${YT[1]}" | tr 'A-Z' 'a-z')
  if [[ "$got" != "$exp" ]]; then
    echo "yt-dlp hash mismatch: expected $exp got $got" >&2
    exit 1
  fi
  mapfile -t FF < <(lock_get ffmpeg "$ff_key")
  if [[ -n "${FF[2]}" ]]; then
    tmp=$(mktemp -d)
    download "${FF[0]}" "$tmp/archive.bin"
    if [[ "${FF[2]}" == zip ]]; then
      unzip -o "$tmp/archive.bin" -d "$tmp/out" >/dev/null
    else
      tar -xf "$tmp/archive.bin" -C "$tmp"
    fi
    src=$(find "$tmp" -type f -name "${FF[3]:-ffmpeg}" | head -n1)
    cp "$src" "$FFMPEG"
    rm -rf "$tmp"
  else
    download "${FF[0]}" "$FFMPEG"
  fi
  chmod +x "$FFMPEG"
  got=$(sha256_of "$FFMPEG")
  exp=$(echo "${FF[1]}" | tr 'A-Z' 'a-z')
  if [[ "$got" != "$exp" ]]; then
    echo "ffmpeg hash mismatch: expected $exp got $got" >&2
    exit 1
  fi
else
  case "$OS-$ARCH" in
    linux-x64) YT_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux" ;;
    linux-arm64) YT_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64" ;;
    macos-*) YT_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos" ;;
  esac
  download "$YT_URL" "$YTDLP"
  chmod +x "$YTDLP"
  case "$OS-$ARCH" in
    linux-x64) download "https://github.com/eugeneware/ffmpeg-static/releases/latest/download/ffmpeg-linux-x64" "$FFMPEG" ;;
    linux-arm64) download "https://github.com/eugeneware/ffmpeg-static/releases/latest/download/ffmpeg-linux-arm64" "$FFMPEG" ;;
    macos-x64) download "https://github.com/eugeneware/ffmpeg-static/releases/latest/download/ffmpeg-darwin-x64" "$FFMPEG" ;;
    macos-arm64) download "https://github.com/eugeneware/ffmpeg-static/releases/latest/download/ffmpeg-darwin-arm64" "$FFMPEG" ;;
  esac
  chmod +x "$FFMPEG"
fi

"$YTDLP" --version | head -n1
"$FFMPEG" -version | head -n1
echo "Engines ready in $CODE"
