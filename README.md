# yt-dlp GUI

A desktop frontend for [yt-dlp](https://github.com/yt-dlp/yt-dlp). Paste a link, pick a quality, download. Built with **Tauri 2 + React + TypeScript**.

Windows-first. Defaults to Chinese UI (English available). Optimized for Bilibili, YouTube, Douyin / TikTok, and the 1000+ sites yt-dlp already supports.

基于 yt-dlp 的图形化下载器：粘贴链接 → 选清晰度 → 下载。默认中文界面，可切换 English。

---

## Features

- **Preview** — title, thumbnail, duration, format list (height + codec). Playlists / 合集 with multi-select.
- **Downloads** — live progress, speed, ETA. Cancel kills the process tree (yt-dlp + ffmpeg). Pause / resume via `--continue`.
- **Queue** — default 2 concurrent jobs; extra URLs wait in line. Survives app restart (in-flight tasks come back paused).
- **Options** — presets (MP4 by default), cookies (browser or Netscape file), HTTP/SOCKS proxy, rate limit, subtitles, SponsorBlock. Live **command preview** you can copy.
- **Toolbox** — subtitles only, thumbnail only, metadata JSON (same download pipeline).
- **Quick download** — skip preview, use the saved default preset (factory: MP4).
- **Engines in-tree** — `code/yt-dlp.exe` + `code/ffmpeg.exe`. The app does **not** assume ffmpeg is on `PATH`.

---

## Screenshots

Run `pnpm tauri dev` and capture a window shot if you want one here (`docs/screenshot.png`).

---

## Requirements

### End users (Windows)

- Windows 10/11 with WebView2 (usually already installed)
- Place these two files next to each other:

| File | Where to get it |
|---|---|
| `code/yt-dlp.exe` | [yt-dlp releases](https://github.com/yt-dlp/yt-dlp/releases) (Windows x64) |
| `code/ffmpeg.exe` | [gyan.dev ffmpeg essentials](https://www.gyan.dev/ffmpeg/builds/) (or any static `ffmpeg.exe`) |

YouTube playlist / channel pages may also need **Deno** or **Node** on `PATH` (yt-dlp 2026+ JS runtime).

### Developers

- [Node.js](https://nodejs.org/) 20+ and [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/) (stable) + Visual Studio C++ build tools
- The two binaries above in `code/`

---

## Quick start (development)

```bash
pnpm install

# put engines here (not committed)
#   code/yt-dlp.exe
#   code/ffmpeg.exe

pnpm tauri dev
```

```bash
# tests
cargo test --manifest-path src-tauri/Cargo.toml
pnpm exec tsc --noEmit

# release installer / exe
pnpm tauri build
```

The packaged app copies `code/yt-dlp.exe` and `code/ffmpeg.exe` as bundle resources. A machine without ffmpeg on `PATH` still works as long as those files were present at build time.

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
code/                 # yt-dlp.exe + ffmpeg.exe (local, gitignored)
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
