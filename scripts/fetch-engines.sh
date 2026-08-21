#!/usr/bin/env bash
# Download yt-dlp + ffmpeg into code/ (gitignored).
# Usage:  bash scripts/fetch-engines.sh [--force] [--arch auto|x64|arm64]
set -euo pipefail

FORCE=0
ARCH=auto

while [[ $# -gt 0 ]]; do
  case "$1" in
    --force|-f) FORCE=1; shift ;;
    --arch)
      ARCH="${2:?--arch needs a value}"
      shift 2
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CODE="$ROOT/code"
mkdir -p "$CODE"

OS_RAW="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$OS_RAW" in
  darwin) OS=macos ;;
  linux) OS=linux ;;
  *)
    echo "unsupported OS: $OS_RAW" >&2
    exit 1
    ;;
esac

if [[ "$ARCH" == auto ]]; then
  case "$(uname -m)" in
    x86_64|amd64) ARCH=x64 ;;
    aarch64|arm64) ARCH=arm64 ;;
    *)
      echo "unsupported arch: $(uname -m)" >&2
      exit 1
      ;;
  esac
fi

file_ok() {
  local f="$1"
  [[ -f "$f" ]] || return 1
  local n
  n="$(wc -c < "$f" | tr -d ' ')"
  [[ "$n" -gt 1000000 ]]
}

download() {
  local url="$1" out="$2"
  echo "Downloading $url"
  curl -L --fail --retry 3 --retry-delay 2 --connect-timeout 30 -o "$out" "$url"
}

YTDLP="$CODE/yt-dlp"
FFMPEG="$CODE/ffmpeg"

if [[ "$FORCE" -eq 0 ]] && file_ok "$YTDLP" && file_ok "$FFMPEG"; then
  echo "Engines already present in code/ (pass --force to re-download)."
  exit 0
fi

case "$OS-$ARCH" in
  linux-x64) YT_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux" ;;
  linux-arm64) YT_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64" ;;
  macos-x64|macos-arm64) YT_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos" ;;
  *)
    echo "no yt-dlp asset for $OS-$ARCH" >&2
    exit 1
    ;;
esac
download "$YT_URL" "$YTDLP"
chmod +x "$YTDLP"

TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

fetch_ffmpeg_btbn() {
  local slug="$1"
  local tar="$TMP/ffmpeg.tar.xz"
  download "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/${slug}" "$tar"
  tar -xJf "$tar" -C "$TMP"
  local bin
  bin="$(find "$TMP" -type f -name ffmpeg | head -n 1)"
  [[ -n "$bin" ]]
  cp "$bin" "$FFMPEG"
}

fetch_ffmpeg_static() {
  local name="$1"
  download "https://github.com/eugeneware/ffmpeg-static/releases/latest/download/${name}" "$FFMPEG"
}

fetch_ffmpeg_evermeet() {
  download "https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip" "$TMP/ffmpeg.zip"
  mkdir -p "$TMP/out"
  unzip -o "$TMP/ffmpeg.zip" -d "$TMP/out" >/dev/null
  local bin
  bin="$(find "$TMP/out" -type f -name ffmpeg | head -n 1)"
  [[ -n "$bin" ]]
  cp "$bin" "$FFMPEG"
}

set +e
case "$OS-$ARCH" in
  linux-x64)
    fetch_ffmpeg_btbn ffmpeg-master-latest-linux64-gpl.tar.xz || fetch_ffmpeg_static ffmpeg-linux-x64
    ;;
  linux-arm64)
    fetch_ffmpeg_btbn ffmpeg-master-latest-linuxarm64-gpl.tar.xz || fetch_ffmpeg_static ffmpeg-linux-arm64
    ;;
  macos-x64)
    fetch_ffmpeg_static ffmpeg-darwin-x64 || fetch_ffmpeg_evermeet
    ;;
  macos-arm64)
    fetch_ffmpeg_static ffmpeg-darwin-arm64
    ;;
  *)
    echo "no ffmpeg asset for $OS-$ARCH" >&2
    exit 1
    ;;
esac
FF_STATUS=$?
set -e
if [[ "$FF_STATUS" -ne 0 ]]; then
  echo "ffmpeg download failed for $OS-$ARCH" >&2
  exit 1
fi
chmod +x "$FFMPEG"

if ! file_ok "$YTDLP" || ! file_ok "$FFMPEG"; then
  echo "engines were not downloaded correctly" >&2
  exit 1
fi

echo "Engines ready in $CODE"
"$YTDLP" --version | head -n 1
"$FFMPEG" -version | head -n 1
