#!/usr/bin/env bash
# Fail the Release job if bundled yt-dlp/ffmpeg are missing, tiny, not executable, or --version fails.
set -euo pipefail
ROOT="${1:-.}"
NAME_YT="yt-dlp"
NAME_FF="ffmpeg"
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) NAME_YT="yt-dlp.exe"; NAME_FF="ffmpeg.exe" ;;
esac

found_yt=0
found_ff=0
while IFS= read -r -d "" f; do
  echo "found $f ($(wc -c < "$f") bytes)"
  if [[ ! -x "$f" && "$(uname -s)" != MINGW* ]]; then
    echo "not executable: $f" >&2
    exit 1
  fi
  n=$(wc -c < "$f" | tr -d ' ')
  if [[ "$n" -lt 1000000 ]]; then
    echo "too small: $f" >&2
    exit 1
  fi
  base="$(basename "$f")"
  if [[ "$base" == yt-dlp* ]]; then
    found_yt=1
    "$f" --version >/dev/null
  else
    found_ff=1
    "$f" -version >/dev/null
  fi
done < <(find "$ROOT" -type f \( -name "$NAME_YT" -o -name "$NAME_FF" \) -path "*/code/*" -print0 2>/dev/null)

if [[ "$found_yt" -eq 0 || "$found_ff" -eq 0 ]]; then
  echo "need both $NAME_YT and $NAME_FF under $ROOT/**/code/ (yt=$found_yt ff=$found_ff)" >&2
  exit 1
fi
echo "bundled engines ok"
