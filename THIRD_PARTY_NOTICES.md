# Third-party notices

yt-dlp GUI is licensed under the MIT License. Its installers form an aggregate
distribution that also contains the independent yt-dlp and FFmpeg executables.
Those executables remain under their own licenses; the MIT License does not
replace or restrict those terms.

Pinned versions, download URLs, source locations, and SHA-256 values are recorded
in `engines.lock.json`. Release assets also contain per-platform engine manifests
with the hashes of the binaries actually bundled.

## yt-dlp

- Version: `2026.08.19`
- Project and source: https://github.com/yt-dlp/yt-dlp/tree/2026.08.19
- License: Unlicense
- Included license text: `licenses/yt-dlp-Unlicense.txt`

## FFmpeg

The bundled FFmpeg executables are distributed under GPL-3.0-only. The complete
GPLv3 text is included at `licenses/FFmpeg-GPL-3.0.txt`.

| Platform | Binary distribution | Corresponding source / notice |
|---|---|---|
| Windows x64 | Gyan FFmpeg 9.0.1 essentials build | https://www.gyan.dev/ffmpeg/builds/ and https://github.com/FFmpeg/FFmpeg/tree/n9.0.1 |
| Linux x64 / arm64 | `eugeneware/ffmpeg-static` b6.1.1 (John Van Sickle builds) | https://github.com/eugeneware/ffmpeg-static/tree/b6.1.1 and https://johnvansickle.com/ffmpeg/ |
| macOS x64 | `eugeneware/ffmpeg-static` b6.1.1 (EverMeet build) | https://github.com/eugeneware/ffmpeg-static/tree/b6.1.1 and https://evermeet.cx/ffmpeg/ |
| macOS arm64 | `eugeneware/ffmpeg-static` b6.1.1 (osxexperts build) | https://github.com/eugeneware/ffmpeg-static/tree/b6.1.1 and https://www.osxexperts.net/ |

FFmpeg source: https://github.com/FFmpeg/FFmpeg

These platform builds may expose different codecs, filters, and configure flags.
Do not assume that a command supported by one platform build is available on all
other platforms.

## Other dependencies

The desktop shell uses Tauri 2, WebView, React, and the Rust/JavaScript packages
listed in `src-tauri/Cargo.lock` and `pnpm-lock.yaml`. Consult those lockfiles and
the corresponding upstream packages for their versions and license terms.
