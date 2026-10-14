# yt-dlp GUI

[![CI](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/ci.yml/badge.svg)](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/ci.yml)
[![Release](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/release.yml/badge.svg)](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/release.yml)
[![GitHub release](https://img.shields.io/github/v/release/kumamon-xu/yt-dlp-GUI)](https://github.com/kumamon-xu/yt-dlp-GUI/releases/latest)

A desktop video downloader powered by [yt-dlp](https://github.com/yt-dlp/yt-dlp), [N_m3u8DL-RE](https://github.com/nilaoda/N_m3u8DL-RE), and FFmpeg. Paste a video page or m3u8 URL, pick a preset, download. Built with **Tauri 2 + React + TypeScript**.

Windows, Linux, and macOS. Defaults to Chinese UI (English available). Optimized for Bilibili, YouTube, Douyin / TikTok, and the 1000+ sites yt-dlp already supports.

基于 yt-dlp 和 N_m3u8DL-RE 的图形化下载器：粘贴视频页面或 m3u8 链接 → 选择预设或具体流 → 下载。支持 Windows / Linux / macOS，内置三个下载工具。默认中文界面，可切换 English。

**Download:** [latest installer](https://github.com/kumamon-xu/yt-dlp-GUI/releases/latest)

## v0.4.1 更新

- 默认启用 RE 点播任务启动检查：抽查首、中、尾分片，验证密钥、AES-128 解密及初始化文件。
- 新增可信备用映射设置；原地址失败且候选镜像通过验证后，生成临时清单自动切换。
- 保留路径、签名参数、字节范围、时间轴以及具体流选择；暂停或取消时终止检查并清理临时清单。
- 修复 HLS 的 SSL 错误提示，避免进度输出和异常堆栈覆盖底层原因。
- 修复 CI 环境关闭 RE 进度渲染导致限速计数无法重置的问题；仅调整 RE 子进程环境。
- 为后续下载途中恢复保留请求上下文及线路计划接口。本版恢复发生在任务启动阶段。

## Features

- **Preview** — title, thumbnail, duration, format list (height + codec). Playlists / 合集 with multi-select.
- **Downloads** — yt-dlp progress, speed and ETA; HLS percentage and engine indicator. Cancel kills the process tree, including the downloader and ffmpeg. Pause / resume uses `--continue` for yt-dlp; RE jobs restart the fragment download.
- **Queue** — default 2 concurrent jobs; extra URLs wait in line. Survives app restart (in-flight tasks come back paused).
- **Options** — presets (MP4 by default), cookies (browser or Netscape file), HTTP/SOCKS proxy, rate limit, subtitles, SponsorBlock. Live **command preview** you can copy.
- **Toolbox** — subtitles only, thumbnail only, metadata JSON (same download pipeline).
- **Quick download** — skip preview, use the saved default preset (factory: MP4).
- **HLS/m3u8** — direct manifest URLs use N_m3u8DL-RE, including audio/video renditions and AES-128 streams. Video pages are resolved when a task starts; a single selected HLS playlist can use RE, while complex track selections stay on yt-dlp. Cookies, Referer, User-Agent, proxy, quality limits, and output directory are carried across.
- **Engines in-tree** — `code/yt-dlp` (+ `.exe` on Windows), `ffmpeg`, and `N_m3u8DL-RE`. Tools can also be selected in Settings.

---

## Requirements

### End users

Install from [Releases](https://github.com/kumamon-xu/yt-dlp-GUI/releases/latest). New builds bundle yt-dlp, ffmpeg, and N_m3u8DL-RE.

| OS | File | Notes |
|---|---|---|
| Windows 10/11 | `yt-dlp.GUI_*_x64-setup.exe` (or `.msi`) | WebView2 is usually already installed |
| Linux x64 / arm64 | `.AppImage` or `.deb` | AppImage: `chmod +x` then run. deb needs WebKitGTK 4.1 |
| macOS | `.dmg` (Apple Silicon and Intel) | Unsigned + unnotarized. If macOS says the app "is damaged", run `xattr -cr "/Applications/yt-dlp GUI.app"` once in Terminal, then open normally (see note below) |

> **macOS Gatekeeper note:** the app is built without an Apple Developer ID, so the linker ad-hoc signs it. A quarantined ad-hoc-signed app is rejected as "damaged" — right-click → Open does **not** bypass this. `xattr -cr` removes the quarantine attribute from the app and the bundled yt-dlp/ffmpeg, after which the app runs normally. On Apple Silicon, download the `aarch64` dmg; the `x64` dmg needs Rosetta 2.

YouTube playlist / channel pages may also need **Deno 2.3+** or **Node.js 22+** on `PATH` (yt-dlp 2026+ JavaScript runtime). The app checks the detected version and falls back from an unsupported Deno to a supported Node.js.

### Developers

- [Node.js](https://nodejs.org/) 22+ and [pnpm](https://pnpm.io/)
- Python 3 for native RE engine downloads, release validation and local HLS smoke tests
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
pnpm install --frozen-lockfile
# Windows: powershell -File scripts/fetch-engines.ps1
# Linux / macOS: bash scripts/fetch-engines.sh
pnpm tauri dev
```

```bash
# tests
cargo test --manifest-path src-tauri/Cargo.toml
pnpm exec tsc --noEmit
pnpm test
python -m unittest scripts/test_prepare_release_assets.py
python scripts/smoke-hls.py  # requires native engines in code/

# release installer / exe
pnpm tauri build
```

The packaged app copies `code/yt-dlp`, `code/ffmpeg`, and `code/N_m3u8DL-RE` as bundle resources (with `.exe` on Windows). The engine fetch scripts download all three, verifying pinned archive/binary hashes in lock mode.

---

## GitHub Actions / releasing

| Workflow | Trigger | What it does |
|---|---|---|
| [CI](.github/workflows/ci.yml) | PR and `main` | version check, `tsc`, `pnpm test`, `cargo fmt`, clippy, `cargo test` |
| [Stable dry run](.github/workflows/stable-dry-run.yml) | relevant PRs or manual | locked, five-platform installer build and validation; never publishes |
| [Release](.github/workflows/release.yml) | `main`, tag `v*`, or manual | five platform jobs upload checked artifacts; one publisher validates the complete set and updates one GitHub Release |

- Push to `main` updates the **nightly** prerelease only. It never deletes or overwrites a `vX.Y.Z` stable release.
- Tag `v*` to publish an immutable **stable** release. Stable engines and license texts are pinned in `engines.lock.json` by SHA-256; the release remains a draft until all five platform artifacts have been validated and uploaded.

```bash
# bump package.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock and src-tauri/tauri.conf.json
git tag -a v0.4.1 -m "yt-dlp GUI v0.4.1"
git push origin v0.4.1
```

CI downloads platform-native yt-dlp + ffmpeg + N_m3u8DL-RE into `code/` before `tauri build`. Installers are unsigned (Windows SmartScreen / macOS Gatekeeper may warn). To sign macOS builds, add `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, and related secrets to the repository, then expose them to the reusable build workflow's installer step.

---

## Usage

1. Paste a video or playlist URL.
2. Click **解析** (or press Enter) — parsing does **not** start on paste alone.
3. Choose a preset in the options panel, or explicitly pick a stream (video-only selections add audio automatically).
4. **下载**. Watch the task pane for progress; **打开文件夹** when done.

**Cookies:** Bilibili 1080p / age-gated YouTube usually need them. Options → browser (close Edge/Chrome first) or a `cookies.txt` file. File wins if both are set.

**Proxy:** e.g. `http://127.0.0.1:7890` or `socks5://127.0.0.1:7890`.

**Several URLs:** whitespace / newline separated, then **快速下载** to enqueue each one.

**预设与设置：**未明确点选流时，下载按钮使用选项面板中的预设；修改面板预设会清除此前的选流。普通选项自动保存不会改变当前会话预设。设置中保存目录、代理或 Cookies 后，新任务立即使用这些值。

**限速：**输入以字节/秒计，`K`、`M`、`G` 使用 1024 倍进位。例如 `500K` 为 500 KiB/s，`1M` 为 1 MiB/s。

### HLS / m3u8

默认开启「自动使用 N_m3u8DL-RE 下载 HLS/m3u8」。粘贴 m3u8 直链即可下载，也可以粘贴 yt-dlp 支持的视频页面：任务启动时会重新解析地址，找到兼容的 HLS 流后自动交给 RE。任务列表显示所用引擎和下载百分比，完成后可打开输出目录。设置里可指定 RE 路径，或关闭自动切换。

- 页面解析依赖 yt-dlp 的站点支持；仅在播放器运行后通过 JavaScript 生成、需要浏览器抓包的地址不保证能解析。
- 视频页面只在能直接使用已选播放列表时切换到 RE，不用主清单重新选流。多轨选择或无法准确映射的情况沿用 yt-dlp，以保留编码、帧率和音轨选择；直接下载主清单时，RE 自动选择音视频轨道并合并。播放列表、音频转换，以及字幕、封面、元数据嵌入和 SponsorBlock 选项也沿用 yt-dlp。
- m3u8 任务支持暂停和取消；继续时重新解析并重新下载分片，不承诺断点续传。旧队列和旧设置可继续使用。
- 视频页面沿用文件名模板，并追加任务 ID 防止并发任务覆盖文件；直链缺少网页元数据，使用清单名称和任务 ID。支持 MP4、MKV、TS 容器。
- Cookies 文件优先于浏览器；浏览器 Cookies 通过 yt-dlp 临时导出，转换为请求头后删除临时文件。解析得到的流 URL 不替换持久化任务中的原始页面 URL。
- 清单可解析时，分片仍可能由其他 CDN 域名提供。出现「SSL/TLS 连接失败」或「首个分片下载失败」时，请检查日志中的分片域名、代理和 Cookies；重新解析同一个清单通常无法修复不可访问的分片 CDN。

设置中默认开启「下载前检查 HLS 点播分片」。在「可信 HLS 备用地址」中添加原地址、备用地址及适用路径前缀，例如 `https://cdn.example.com` → `https://backup.example.com`，路径 `/videos/`。镜像必须提供同一媒体、密钥和字节范围；程序只尝试明确配置的映射，不从域名名称推断镜像。HTTP 401/403/429 会提示认证或限流原因，不通过换域名掩盖这些错误。请勿将带认证信息或签名参数的 URL 填进映射设置。

检查使用任务的代理和请求头，通过实际 GET 请求验证媒体内容，单个分片样本最多读取 64 KiB。主清单按照预设清晰度限制和带宽确定视频流，并保留关联音轨；明确选择的单一媒体流仍使用其原始 URL。密钥和初始化文件另行检查，必要时验证 AES-128 解密。验证成功后使用私有临时清单，结束、暂停或取消后清理，队列中仍保存原始任务 URL。每次继续任务会重新检查，不缓存长期有效的签名地址。

本版仅在 RE 任务启动阶段检查和修复。直播、变量清单、其他加密方式及加密字节范围流沿用 RE；复杂多轨选择和 yt-dlp 后处理任务沿用 yt-dlp。镜像需要不同 Cookie/Authorization 时不会跨域转发原认证头；这类请求需要后续的分片请求服务。抽样检查不能保证未抽查分片可用，下载途中自动恢复尚未实现。可关闭启动检查以沿用此前的直接下载方式。

本地完整验证（需要 `code/` 中的三个真实引擎）：`python scripts/smoke-hls.py`。它生成带独立音轨、同分辨率不同编码/帧率及 AES-128 加密的 HLS，使用要求 Cookies/Referer 的本地服务器，验证启动检查、普通/AES-128 分片的可信镜像切换、页面解析、具体流选择、多轨回退和 RE 的真实限速。

---

## Project layout

`.gitignore` 排除依赖、构建产物、本地笔记、认证配置、Cookies、下载媒体和 HLS 临时文件。源码、测试、依赖锁文件、引擎锁文件及分发所需的许可证保留在仓库中。发布产物通过 GitHub Releases 分发，不提交到源码仓库。

```
code/                 # yt-dlp + ffmpeg + N_m3u8DL-RE (local, gitignored)
src/                  # React UI
src-tauri/src/        # Rust: process pool, arg builder, queue
  command.rs          # pure yt-dlp argv builder
  parser.rs           # YDLP| progress lines + error mapping
  tasks.rs            # spawn / kill tree / concurrency
  info.rs             # -J preview
  m3u8.rs             # HLS routing, RE arguments and progress
  hls_preflight.rs    # VOD probes, trusted mappings and recovery plans
  config.rs           # settings JSON + folder picker
```

---

## Disclaimer

Use this tool to download content you have the right to obtain. You are responsible for complying with site terms and copyright law.

仅用于下载你有权获取的内容。请遵守各平台条款与著作权法。

---

## License

The yt-dlp GUI source is licensed under the [MIT License](LICENSE).

Installers also bundle yt-dlp (Unlicense), GPLv3 FFmpeg builds, and N_m3u8DL-RE (MIT). Their source links, license texts, platform distributors, and pinned hashes are documented in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), `licenses/`, and `engines.lock.json`.
