# yt-dlp GUI

[![CI](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/ci.yml/badge.svg)](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/ci.yml)
[![Release](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/release.yml/badge.svg)](https://github.com/kumamon-xu/yt-dlp-GUI/actions/workflows/release.yml)
[![GitHub release](https://img.shields.io/github/v/release/kumamon-xu/yt-dlp-GUI)](https://github.com/kumamon-xu/yt-dlp-GUI/releases/latest)

A desktop video downloader powered by [yt-dlp](https://github.com/yt-dlp/yt-dlp), [N_m3u8DL-RE](https://github.com/nilaoda/N_m3u8DL-RE), and FFmpeg. Paste a video page or m3u8 URL, pick a preset, download. Built with **Tauri 2 + React + TypeScript**.

Windows, Linux, and macOS. Defaults to Chinese UI (English available). Optimized for Bilibili, YouTube, Douyin / TikTok, and the 1000+ sites yt-dlp already supports.

基于 yt-dlp 和 N_m3u8DL-RE 的图形化下载器：粘贴视频页面或 m3u8 链接 → 选择预设或具体流 → 下载。支持 Windows / Linux / macOS，内置三个下载工具。默认中文界面，可切换 English。

**Download:** [latest installer](https://github.com/kumamon-xu/yt-dlp-GUI/releases/latest)

## v0.6.0 更新

- 移除所有内置站点换域规则；从原清单、标准 Content Steering 和当前播放器请求自动分析。
- 新增隔离的 Edge / Chrome / Chromium 辅助解析，支持 JavaScript 和嵌入播放器；预览及任务在 yt-dlp 解析失败后使用同一回退流程。
- 点播失败后重试、刷新原清单和原页面，核对所选流及有序分片路径、时长和加密信息，拒绝把其他视频当成备用。
- 浏览器来源通过本机资源服务交给 RE：清单、密钥和分片分别应用请求头；原生 TLS 或认证请求失败时使用浏览器网络栈取回资源，保留证书校验。
- HLS 设置窗口实时显示解析、检查、重试和失败日志，可指定浏览器程序；没有可验证的线路时明确报错。
- 增加动态播放器、嵌入页面、认证头隔离、清单刷新、错误视频拒绝、资源服务清理和真实 RE 字节范围下载测试。

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
git tag -a v0.6.0 -m "yt-dlp GUI v0.6.0"
git push origin v0.6.0
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

默认开启「自动使用 N_m3u8DL-RE 下载 HLS/m3u8」。m3u8 直链直接进入 HLS 流程，视频页面先由 yt-dlp 解析。yt-dlp 失败或超时时，自动分析可使用安装的 Edge、Chrome 或 Chromium 观察当前播放器的清单和请求头，支持 JavaScript 和嵌入播放器。设置 → HLS 线路与备用映射 → 浏览器程序路径可手动指定，留空自动查找。浏览器不随安装包分发。

具体流选择仍优先保留 yt-dlp 的结果；无法准确对应的多轨、自定义格式和后处理任务继续使用 yt-dlp。浏览器回退只选当前播放器，不采集页面上的其他剧集作为备用。动态解析的预览提供基础 HLS 信息，下载时重新解析，不保存临时签名 URL，也不把发现的地址写入永久映射。

**检查与恢复：**点播启动时抽查首、中、尾分片及所有密钥和初始化文件，验证媒体内容和必要的 AES-128 解密。TLS、网络及可恢复服务器错误会有限重试，再刷新原清单及原视频页面。刷新时检查所选流的属性、有序分片路径、时长、序号、字节范围和加密信息。标准备用线路由主清单稳定流 ID 和 Content Steering 提供，使用前同样验证。没有内置站点换域规则，不猜随机子域名，不仅凭 HTTP 200 认定媒体有效；站点不提供可验证的来源时仍可能需要手动映射。

**浏览器网络通道：**浏览器来源的点播通过任务专属的 `127.0.0.1` 资源服务交给 RE。服务只暴露已登记资源，使用随机标识，原 URL 和认证信息留在内存中。清单专用认证头不会复制到分片；一致的分片请求头模式仅用于同源、同资源目录和同类型资源，Cookie 保留域名与路径范围。原生连接或认证请求失败时，由原播放器页面的浏览器上下文取回资源，保留 TLS 校验和浏览器跨域策略。

设置中明确填写的代理用于外部请求；RE 访问本机资源服务时直接连接本机。未填写代理时不使用系统 HTTP 代理，系统 TUN 的网络路由仍生效。带用户名密码的显式代理暂不支持浏览器通道。浏览器使用独立临时配置，不打开现有用户配置；下载解析可导入所选 Cookies 文件或 yt-dlp 导出的浏览器 Cookies。完成、暂停、取消或解析失败后清理本机服务、浏览器进程和临时文件。

**手动备用作为补充：**例如 `https://cdn.example.com` → `https://backup.example.com`。路径 `/videos/` 匹配该目录下多个视频；留空或 `/` 匹配该源地址的全部路径，只替换源地址并保留资源路径和签名参数。不同视频不需要分别建立规则，但源地址必须准确匹配，镜像必须提供同一媒体、密钥和字节范围。HTTP 401/403/429 不通过猜域名绕过；请检查 Cookies、授权、限流或清单有效期。映射中不要填写认证信息或签名 URL。

「分析并检查链接」使用当前设置草稿的代理、Cookies 和清晰度预设（音频转换预设按最佳视频分析），实时显示检查过程和涉及的源地址。只加载页面、脚本、清单及有限资源样本，不下载完整媒体、不创建队列任务。可取消分析，关闭弹窗也会取消。每个分片检查样本最多 64 KiB。独立映射表格可搜索、滚动和单独保存。

**边界：**浏览器通道目前支持可验证的点播，单个完整资源上限 32 MiB；它会缓存一个资源后交给 RE，下载吞吐和瞬时流量会受缓冲影响。自动分析不能保证服务器永久可达，也不能恢复没有有效来源的地址。需要手动播放、登录或验证码的页面可能无法自动解析。直播、变量、复杂加密及加密字节范围流仅在原有直接模式中交给 RE；浏览器来源遇到这些情况明确提示不支持。下载中的同一资源可通过浏览器网络通道及 RE 重试，但清单过期后的重新解析、下载中跨线路切换仍留作后续工作。浏览器来源始终需要点播检查来建立资源服务；关闭启动检查仅让原生来源沿用直接模式。

**验证：**`python scripts/smoke-hls.py` 使用本地生成的视频验证真实引擎、音轨/编码/帧率选择、AES-128、备用线路、Steering 和限速。有已安装浏览器时还验证动态播放器、认证头隔离及本机服务的完整分片和字节范围下载，最后使用 FFmpeg 检查输出 MP4。额外的本地嵌入播放器检查：`cargo test --manifest-path src-tauri/Cargo.toml real_browser_observes -- --ignored`。远程回归仅通过测试环境变量提供地址，不固化真实站点规则。

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
  hls_discovery.rs    # provider-declared alternatives and site adapters
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
