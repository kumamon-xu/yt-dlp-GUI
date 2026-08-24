# yt-dlp GUI

[![CI](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/ci.yml/badge.svg)](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/ci.yml)
[![Release](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/release.yml/badge.svg)](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/release.yml)
[![GitHub release](https://img.shields.io/github/v/release/kumamon-xu/yt-dlp-GUI)](https://github.com/kumamon-xu/yt-dlp-GUI/releases/latest)

A desktop frontend for [yt-dlp](https://github.com/yt-dlp/yt-dlp). Paste a link, pick a quality, download. Built with **Tauri 2 + React + TypeScript**.

Windows, Linux, and macOS. Defaults to Chinese UI (English available). Optimized for Bilibili, YouTube, Douyin / TikTok, and the 1000+ sites yt-dlp already supports.

基于 yt-dlp 的图形化下载器：粘贴链接 → 选清晰度 → 下载。支持 Windows / Linux / macOS。默认中文界面，可切换 English。

**Download:** [latest installer](https://github.com/kumamon-xu/yt-dlp-GUI/releases/latest)

---

## Features

- **Preview** — title, thumbnail, duration, format list (height + codec). Playlists / 合集 with multi-select.
- **Downloads** — live progress, speed, ETA. Cancel kills the process tree (yt-dlp + ffmpeg). Pause / resume via `--continue`.
- **Queue** — default 2 concurrent jobs; extra URLs wait in line. Survives app restart (in-flight tasks come back paused).
- **Options** — presets (MP4 by default), cookies (browser or Netscape file), HTTP/SOCKS proxy, rate limit, subtitles, SponsorBlock. Live **command preview** you can copy.
- **Toolbox** — subtitles only, thumbnail only, metadata JSON (same download pipeline).
- **Quick download** — skip preview, use the saved default preset (factory: MP4).
- **Engines in-tree** — `code/yt-dlp` (+ `.exe` on Windows) and `ffmpeg`. The app does **not** assume ffmpeg is on `PATH`.

---

## Screenshots

Run `pnpm tauri dev` and capture a window shot if you want one here (`docs/screenshot.png`).

---

## Requirements

### End users

Install from [Releases](https://github.com/kumamon-xu/yt-dlp-GUI/releases/latest). yt-dlp and ffmpeg are **bundled**.

| OS | File | Notes |
|---|---|---|
| Windows 10/11 | `yt-dlp.GUI_*_x64-setup.exe` (or `.msi`) | WebView2 is usually already installed |
| Linux x64 / arm64 | `.AppImage` or `.deb` | AppImage: `chmod +x` then run. deb needs WebKitGTK 4.1 |
| macOS | `.dmg` (Apple Silicon and Intel) | Unsigned: right-click the app → Open (Gatekeeper) |

YouTube playlist / channel pages may also need **Deno** or **Node** on `PATH` (yt-dlp 2026+ JS runtime).

### Developers

- [Node.js](https://nodejs.org/) 20+ and [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/) (stable). Windows: Visual Studio C++ build tools. Linux: WebKitGTK 4.1 + `librsvg2-dev` + `patchelf`
- Engines in `code/` (not committed):

```powershell
# Windows
powershell -File scripts/fetch-engines.ps1
```

```bash
# Linux / macOS
bash scripts/fetch-engines.sh
```

---

## Quick start (development)

```bash
pnpm install
# Windows: powershell -File scripts/fetch-engines.ps1
# Linux / macOS: bash scripts/fetch-engines.sh
pnpm tauri dev
```

```bash
# tests
cargo test --manifest-path src-tauri/Cargo.toml
pnpm exec tsc --noEmit
pnpm test

# release installer / exe
pnpm tauri build
```

The packaged app copies `code/yt-dlp` and `code/ffmpeg` as bundle resources. A machine without ffmpeg on `PATH` still works as long as those files were present at build time.

---

## GitHub Actions / releasing

| Workflow | Trigger | What it does |
|---|---|---|
| [CI](.github/workflows/ci.yml) | PR and `main` | version check, `tsc`, `pnpm test`, `cargo fmt`, clippy, `cargo test` |
| [Release](.github/workflows/release.yml) | `main`, tag `v*`, or manual | Windows + Linux (x64/arm64) + macOS (arm64/x64) → one GitHub Release |

- Push to `main` updates the **nightly** prerelease only. It never deletes or overwrites a `vX.Y.Z` stable release.
- Tag `v*` to publish an immutable **stable** release (GitHub Latest). Engines are pinned in `engines.lock.json` (sha256).

```bash
# bump version in package.json, src-tauri/Cargo.toml, src-tauri/tauri.conf.json
git tag v0.2.0
git push origin v0.2.0
```

CI downloads platform-native yt-dlp + ffmpeg into `code/` before `tauri build`. Installers are unsigned (Windows SmartScreen / macOS Gatekeeper may warn). To sign macOS builds, add `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, and related secrets to the repo, then pass them as `env` on the `tauri-action` step.

---

## Usage

1. Paste a video or playlist URL.
2. Click **解析** (or press Enter) — parsing does **not** start on paste alone.
3. Pick a quality (video-only DASH streams are merged with audio automatically).
4. **下载**. Watch the task pane for progress; **打开文件夹** when done.

**Cookies:** Bilibili 1080p / age-gated YouTube usually need them. Options → browser (close Edge/Chrome first) or a `cookies.txt` file. File wins if both are set.

**Proxy:** e.g. `http://127.0.0.1:7890` or `socks5://127.0.0.1:7890`.

**Several URLs:** whitespace / newline separated, then **快速下载** to enqueue each one.

---

## Project layout

```
code/                 # yt-dlp + ffmpeg (local, gitignored)
src/                  # React UI
src-tauri/src/        # Rust: process pool, arg builder, queue
  command.rs          # pure yt-dlp argv builder
  parser.rs           # YDLP| progress lines + error mapping
  tasks.rs            # spawn / kill tree / concurrency
  info.rs             # -J preview
  config.rs           # settings JSON + folder picker
```

---

## Disclaimer

This tool only wraps yt-dlp. Use it to download content you have the right to obtain. You are responsible for complying with site terms and copyright law.

仅用于下载你有权获取的内容。请遵守各平台条款与著作权法。

---

## License

No license file is attached yet. Treat the source as source-available until the author adds one.
