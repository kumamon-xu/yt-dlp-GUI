# Third-party notices

This project is a GUI wrapper. Installers **bundle** copies of yt-dlp and FFmpeg.
Those binaries keep their own licenses; this file records what we ship.

## yt-dlp

- Source: https://github.com/yt-dlp/yt-dlp
- License: Unlicense (public domain dedication) — see upstream `LICENSE`
- Pinned version / hashes: `engines.lock.json` (`yt-dlp.version` and per-arch `sha256`)

## FFmpeg

Windows and Unix currently use **different upstream builds** (not the same version family):

| Platform | Build | License notes |
|---|---|---|
| Windows x64 | Gyan essentials 9.0.1 (`engines.lock.json` → `ffmpeg.windows-x64`) | FFmpeg is LGPL/GPL depending on configure flags. Gyan “essentials” builds are typically LGPL. See https://www.gyan.dev/ffmpeg/builds/ |
| Linux / macOS | eugeneware/ffmpeg-static `b6.1.1` | Static builds; treat as GPL-capable unless you verify configure. https://github.com/eugeneware/ffmpeg-static |

Do not assume identical codecs/filters across Windows vs Unix until a dedicated merge regression suite exists.

## Other runtime / UI dependencies

The desktop shell uses Tauri 2, WebView, React, and crates listed in `src-tauri/Cargo.lock` / `pnpm-lock.yaml`. Consult those lockfiles for versions.
