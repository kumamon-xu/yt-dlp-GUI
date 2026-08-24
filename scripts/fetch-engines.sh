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
  python3 - "$LOCK" "$1" "$2" "$3" <<'PY'
import json,sys
lock=json.load(open(sys.argv[1]))
tool, key, field=sys.argv[2], sys.argv[3], sys.argv[4]
node=lock[tool][key]
value=node.get(field)
if value is None:
    value=""
print(value)
PY
}

tool_get() {
  python3 - "$LOCK" "$1" "$2" <<'PY'
import json,sys
lock=json.load(open(sys.argv[1]))
value=lock[sys.argv[2]].get(sys.argv[3])
if value is None:
    value=""
print(value)
PY
}

verify_license_file() {
  license_file="$(tool_get "$1" licenseFile)"
  license_sha="$(tool_get "$1" licenseSha256 | tr 'A-Z' 'a-z')"
  if [[ -z "$license_file" || -z "$license_sha" ]]; then
    echo "missing license metadata for $1" >&2
    exit 1
  fi
  license_path="$ROOT/$license_file"
  if [[ ! -f "$license_path" ]]; then
    echo "missing license file: $license_path" >&2
    exit 1
  fi
  got=$(sha256_of "$license_path")
  if [[ "$got" != "$license_sha" ]]; then
    echo "$1 license hash mismatch: expected $license_sha got $got" >&2
    exit 1
  fi
}

YTDLP="$CODE/yt-dlp"
FFMPEG="$CODE/ffmpeg"

if [[ "$MODE" == lock ]]; then
  verify_license_file yt-dlp
  verify_license_file ffmpeg
fi

yt_key="$OS-$ARCH"
[[ "$OS" == macos ]] && yt_key=macos
ff_key="$OS-$ARCH"
[[ "$OS" == macos ]] && ff_key="macos-$ARCH"

if [[ "$FORCE" != "1" && -x "$YTDLP" && -x "$FFMPEG" ]]; then
  yt_sz=$(wc -c < "$YTDLP" | tr -d ' ')
  ff_sz=$(wc -c < "$FFMPEG" | tr -d ' ')
  if [[ "$yt_sz" -gt 1000000 && "$ff_sz" -gt 1000000 ]]; then
    existing_valid=1
    if [[ "$MODE" == lock ]]; then
      yt_expected=$(lock_get yt-dlp "$yt_key" sha256 | tr 'A-Z' 'a-z')
      ff_expected=$(lock_get ffmpeg "$ff_key" sha256 | tr 'A-Z' 'a-z')
      if [[ "$(sha256_of "$YTDLP")" != "$yt_expected" || "$(sha256_of "$FFMPEG")" != "$ff_expected" ]]; then
        existing_valid=0
        echo "existing engines do not match lock; re-downloading"
      fi
    fi
    if [[ "$existing_valid" == 1 ]]; then
      echo "engines already in $CODE (pass --force to re-download)"
      "$YTDLP" --version | head -n1
      "$FFMPEG" -version | head -n1
      exit 0
    fi
  fi
fi

download() {
  echo "Downloading $1"
  curl -L --fail --retry 3 --retry-delay 2 --connect-timeout 30 -o "$2" "$1"
}

if [[ "$MODE" == lock ]]; then
  YT_URL="$(lock_get yt-dlp "$yt_key" url)"
  YT_SHA="$(lock_get yt-dlp "$yt_key" sha256)"
  download "$YT_URL" "$YTDLP"
  chmod +x "$YTDLP"
  got=$(sha256_of "$YTDLP")
  exp=$(echo "$YT_SHA" | tr 'A-Z' 'a-z')
  if [[ "$got" != "$exp" ]]; then
    echo "yt-dlp hash mismatch: expected $exp got $got" >&2
    exit 1
  fi
  FF_URL="$(lock_get ffmpeg "$ff_key" url)"
  FF_SHA="$(lock_get ffmpeg "$ff_key" sha256)"
  FF_ARCHIVE="$(lock_get ffmpeg "$ff_key" archive)"
  FF_MEMBER="$(lock_get ffmpeg "$ff_key" member)"
  if [[ -n "$FF_ARCHIVE" ]]; then
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    download "$FF_URL" "$tmp/archive.bin"
    if [[ "$FF_ARCHIVE" == zip ]]; then
      unzip -o "$tmp/archive.bin" -d "$tmp/out" >/dev/null
    else
      tar -xf "$tmp/archive.bin" -C "$tmp"
    fi
    src=$(find "$tmp" -type f -name "${FF_MEMBER:-ffmpeg}" -print -quit)
    if [[ -z "$src" ]]; then
      echo "ffmpeg member not found: ${FF_MEMBER:-ffmpeg}" >&2
      exit 1
    fi
    cp "$src" "$FFMPEG"
    rm -rf "$tmp"
    trap - EXIT
  else
    download "$FF_URL" "$FFMPEG"
  fi
  chmod +x "$FFMPEG"
  got=$(sha256_of "$FFMPEG")
  exp=$(echo "$FF_SHA" | tr 'A-Z' 'a-z')
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
