# yt-dlp GUI 开发文档（Vibe Coding 版）

> 目标：基于现有 `code/yt-dlp.exe`（2026.08.19，Windows x64），做一个可解析 1000+ 网站视频的图形化下载器，重点覆盖 B站/YouTube/抖音/TikTok 等平台。
> 本文档是"喂给 AI 编程助手"的开发蓝图：先给 AI 看调研结论，再按 §9 里程碑逐段实现。
>
> 撰写日期：2026-07-21 ｜ 本地环境：Windows ｜ Python 3.14 / Node 24 / pnpm 10 / Rust 1.89 / ffmpeg 8.1

---

## 1. 调研：主流开源 yt-dlp GUI 对比

| 项目 | 技术栈 | 状态 | 值得借鉴的点 |
|---|---|---|---|
| [imsyy/yt-dlp-gui](https://github.com/imsyy/yt-dlp-gui) | Tauri 2 + Vue3 + TS | 活跃 | 粘贴 URL 即时预览（标题/封面/时长/格式）；下载队列（暂停/继续/取消）；实时速度+ETA；播放列表整选/单选；工具盒（封面下载器、字幕提取 SRT/VTT/ASS/LRC+双语合并、直播聊天归档）；浏览器扩展"一键发送"；安装包仅 ~10MB；仓库内含 `CLAUDE.md`+`yt-dlp.md`（AI 友好文档，此做法值得抄） |
| [dsymbol/yt-dlp-gui](https://github.com/dsymbol/yt-dlp-gui) | PySide6 | 已归档(2026-03) | **预设(presets)机制**：`config.toml` 定义预设参数组，如 `mp4_thumbnail = ["-f","bv*[vcodec^=avc]+ba[ext=m4a]/b","--embed-thumbnail"]`，基础参数+预设拼接；`debug.log` 排错；Windows 便携 ZIP 发布 |
| [wshhja/yt-dlp-gui](https://github.com/wshhja/yt-dlp-gui) | C++ Qt6 | 活跃 | **实时命令预览**（改配置即时生成完整 yt-dlp 命令，可复制，对学习调试极友好）；内置终端控制台；SponsorBlock；代理/UA/浏览器 Cookies；一键检查引擎更新；自动记忆设置 |
| persepolis | Python + Web | 活跃 | 队列管理最完善，但架构偏服务器，不适合轻 GUI |
| cornradio/ytdlpgui | C# | 维护少 | 极简+代理支持，说明"极简+代理"是长尾刚需 |

**社区痛点（Reddit / CSDN / 知乎综合）：**
1. 命令行门槛高，普通用户只想"粘贴链接 → 选清晰度 → 下载"；
2. **Cookies 是大头**：B 站 1080P/高帧率、YouTube 会员内容都需要，GUI 必须做"从浏览器导入 Cookies"；
3. 播放列表/合集下载（B 站合集、YouTube 播放列表、抖音合集）；
4. 下载前先看元数据（标题、简介、格式列表）再决定下哪个；
5. 深色主题、低内存占用；
6. 代理支持（科学上网场景）；
7. 断点续传/队列持久化（程序崩溃后任务不丢）。

**定位结论 = imsyy 的现代体验 + dsymbol 的预设机制 + wshhja 的实时命令预览。**
差异化重点：中文平台（B站/抖音）体验、预设模板库、命令可视化。

---

## 2. 核心技术原理：GUI ↔ yt-dlp 通信协议（全部本地验证 ✅）

**设计原则：GUI 是"壳"，yt-dlp.exe 是"引擎"。** 子进程 + 命令行参数交互，不 import Python 库。
好处：yt-dlp 周更，GUI 无需重打包（引擎可 `-U` 自更新）；崩溃隔离；前端技术栈自由。

### 2.1 能力①：元数据预览（不下载）
```bash
yt-dlp -J --no-playlist "URL"   # 单视频：完整 JSON（title/thumbnail/duration/uploader/formats[]）
yt-dlp -J "URL"                 # 播放列表：单个 JSON，含 "playlist": [ {...}, ... ]
yt-dlp -F "URL"                 # 人类可读格式表格（备用）
```
`formats[]` 每项含 `format_id/ext/resolution/vcodec/acodec/filesize/fps/dynamic_range/protocol`，是清晰度选择器的数据源。

### 2.2 能力②：自定义进度行（进度条的关键）⭐
`--progress-template` 输出任意模板：`info.*`=视频字段，`progress.*`=进度字段；`--progress-delta 0.3` 控制刷新频率。实测：
```bash
yt-dlp --newline --no-color \
  --progress-template "download:YDLP|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.speed)s|%(progress.eta)s|%(info.title)s" \
  --progress-delta 0.3 -o "out.%(ext)s" "https://www.w3schools.com/html/mov_bbb.mp4"
```
```
[download] Destination: ...\out.mp4
YDLP|downloading|1024|788493|NA|NA|mov_bbb
YDLP|downloading|130048|788493|5701601.85|0|mov_bbb
YDLP|finished|788493|788493|3023601.78|NA|mov_bbb
```
**解析规则（写进 parser.rs 注释）：**
- 只处理以 `YDLP|` 开头的 stdout 行 → 进度事件；其余 stdout/stderr → 日志控制台。
- `status`：`downloading` | `finished`（后处理阶段用 `postprocess:` 前缀的模板单独监听）。
- `total_bytes` 可能缺失 → 回退 `total_bytes_estimate`；`speed`/`eta` 可能为 `NA`。
- 字段用 `|` 分隔；title 放最后，解析时按前 6 个 `|` 切分，剩余全归 title。

### 2.3 能力③：阶段化字段打印
```bash
yt-dlp -O "%(title)s|%(id)s|%(duration)s|%(thumbnail)s" "URL"  # 只打印不下载
yt-dlp -O "post_process:%(filepath)s" "URL"                    # 后处理完成时打印最终文件路径
```
`WHEN` 取值：`before_dl` / `post_process` / `after_move` / `postprocessor:PP名` 等（`yt-dlp -h` 可查）。
**用途**：下载完成用 `post_process:%(filepath)s` 拿最终文件绝对路径 → "打开所在文件夹"。

### 2.4 启动子进程的硬性要求
```
--no-color           # 关 ANSI 颜色码
--newline            # 按行缓冲（GUI 读管道必须）
PYTHONUTF8=1         # 子进程环境变量！Windows GBK 控制台会搞坏 UTF-8 中文标题
--windows-filenames  # 输出文件名强制 Windows 兼容
```
Rust 侧 `tokio::process::Command`，参数数组传递，**禁止**拼 shell 字符串（URL 里的空格/引号/`&` 都是坑）。

### 2.5 暂停 / 继续 / 取消（yt-dlp 无原生暂停）
- **取消**：杀进程树。Windows `taskkill /F /T /PID <pid>`；跨平台 process group kill。
- **继续**：用**完全相同的命令**重跑 + `--continue`，自动续传 `.part`（DASH/HLS 分片同样支持）。
- 因此任务必须**持久化保存完整参数**，不只是 URL。
- 界面"暂停" = 杀进程 + 任务标记 `paused`；"继续" = 存参 + `--continue` 重启。

### 2.6 Cookies（中文平台刚需）
```bash
yt-dlp --cookies-from-browser edge "URL"    # edge/chrome/brave/firefox/...
yt-dlp --cookies D:/cookies.txt "URL"       # Netscape 格式文件
```
UI 两个入口：① 下拉选浏览器（默认 Edge）② 导入 cookies.txt。
注意：从浏览器读 Cookies 时**目标浏览器不能运行**（cookie 库被锁）→ UI 提示"请关闭浏览器后重试"。

### 2.7 ffmpeg 依赖
合并音视频（YouTube/B站 DASH）、提取音频、转封装、嵌入封面都依赖 ffmpeg。
启动时 `ffmpeg -version` 检测：缺失 → 设置页红字警告、相关预设禁用、格式选择回退 `b`（单文件最佳不合并）。
发布策略：引导用户自装（gyan.dev essentials）或捆绑进便携目录。

### 2.8 引擎自更新
```bash
yt-dlp -U          # 引擎自更新（stable）
yt-dlp --version   # 与 GitHub /releases/latest 对比提示
```

---

## 3. 技术选型

**首选：Tauri 2 + React 18 + TypeScript + Vite + TailwindCSS + shadcn/ui**

| 维度 | 评估 |
|---|---|
| 包体积 | 安装包 ~10-15MB（Electron 80MB+） |
| AI 友好度 | React/TS 训练语料最丰富，vibe coding 效率最高；Rust 侧仅 ~500-800 行（进程管理），AI 可稳定生成 |
| 进程管理 | `tokio::process` 天然适合多任务子进程池 + 行式 stdout 读取 |
| 本机条件 | Node 24 / pnpm 10 / Rust 1.89 全就绪 ✅ |
| 生态 | tauri-plugin-store(持久化) / tauri-plugin-dialog(选目录) / tauri-plugin-updater(自更新) / 托盘 |

备选（不推荐但记录理由）：
- **Electron + React**：零 Rust 门槛，但包大内存高。Rust 侧卡壳时的降级方案。
- **PySide6**：同语言可直接 `import yt_dlp` 用 `progress_hooks`（回调比管道更优雅），但 PyInstaller 打包坑多、跨平台一致性差。

---

## 4. 系统架构

```
┌──────────────────────── Tauri 窗口 (WebView2) ────────────────────────────┐
│  React + TS + Tailwind + shadcn/ui + Zustand                              │
│  ┌──────────┐ ┌────────────┐ ┌──────────┐ ┌─────────────┐ ┌───────────┐  │
│  │URL 输入  │ │预览卡片     │ │选项面板  │ │队列/任务表  │ │日志控制台 │  │
│  │(支持批量)│ │标题/封面/时长│ │预设+高级 │ │进度/速度/ETA│ │(实时滚动) │  │
│  └──────────┘ └────────────┘ └──────────┘ └─────────────┘ └───────────┘  │
│  ┌────────────────────────────────────────────────────────────────────┐   │
│  │ 命令预览条：实时显示拼好的完整 yt-dlp 命令，可一键复制              │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│  设置页：下载目录/文件名模板/并发数/代理/Cookies/引擎路径/更新            │
└───────────────────────┬───────────────────────────────────────────────────┘
                        │ invoke() 命令 / emit() 事件
┌───────────────────────▼───────────────────────────────────────────────────┐
│  Rust 核心 (src-tauri/src/)                                               │
│  ├─ command.rs   参数构造器：TaskConfig → Vec<String>（纯函数，可单测）   │
│  ├─ process.rs   进程池：spawn / kill 进程树 / 并发上限 / 行式读 stdout   │
│  ├─ parser.rs    输出解析：YDLP| 进度行 / 日志行 / 错误模式映射           │
│  ├─ queue.rs     任务队列状态机 + JSON 持久化 + 启动恢复                 │
│  └─ config.rs    设置读写、ffmpeg/引擎检测、引擎更新                     │
└───────────────────────┬───────────────────────────────────────────────────┘
                        │ tokio::process（参数数组，非 shell）
┌───────────────────────▼───────────────────────────────────────────────────┐
│  code/yt-dlp.exe (2026.08.19, 可 -U 自更新)  +  ffmpeg.exe (PATH/捆绑)    │
└───────────────────────────────────────────────────────────────────────────┘
```

**数据流（下载一个视频）：**
1. 前端 URL+选项 → `invoke("start_task")` → 参数构造（纯函数）→ 入队 → 进程池 spawn；
2. 进程池逐行读 stdout：`YDLP|...` → `emit("task_progress")`；其余 → `emit("task_log")`；
3. 收到 `post_process:%(filepath)s` → 任务 done，记录最终文件路径；
4. 退出码 ≠ 0 → 解析 stderr 尾部 → 映射友好提示（§7.6）→ failed（可一键重试）。

---

## 5. 数据模型（TypeScript）

```ts
type TaskStatus = "queued" | "downloading" | "postprocess" | "paused"
                | "done" | "failed" | "canceled";

interface DownloadOptions {
  preset: "best" | "1080p" | "720p" | "audio_mp3" | "audio_m4a" | "custom";
  customFormat?: string;        // 自定义 -f 表达式
  audioQuality?: string;        // "192K" / "0"(vbr best)
  mergeFormat?: string;         // mp4 / mkv / mov
  outDir?: string;              // 覆盖全局目录
  outTemplate?: string;         // 默认 "%(title)s [%(id)s].%(ext)s"
  embedThumbnail?: boolean;     // --embed-thumbnail
  embedMetadata?: boolean;      // --embed-metadata
  writeSubs?: boolean; subLangs?: string;  // --write-subs --sub-langs "zh.*,en.*"
  embedSubs?: boolean;          // --embed-subs
  sponsorblock?: boolean;       // --sponsorblock-remove all
  concurrentFragments?: number; // -N，默认 4
  limitRate?: string;           // -r，如 "1M"
  noPlaylist?: boolean;         // --no-playlist
  playlistItems?: string;       // --playlist-items "1-5,10"
  cookiesBrowser?: string;      // --cookies-from-browser edge
  cookiesFile?: string;         // --cookies path
  proxy?: string;               // --proxy http://127.0.0.1:7890
  extractorArgs?: string;       // 高级：--extractor-args
}

interface Task {
  id: string; url: string;
  title?: string; thumbnail?: string; duration?: number; uploader?: string;
  options: DownloadOptions;
  args: string[];               // ★ 完整参数快照（续传/重试/命令预览都靠它）
  status: TaskStatus;
  progress?: { downloaded: number; total: number; speed?: number; eta?: number };
  filePath?: string;            // 最终文件路径（post_process 行回填）
  error?: string;               // 友好错误信息
  createdAt: number;
}
```

---

## 6. 目录结构

```
yt-dlp GUI/
├─ code/yt-dlp.exe          # 引擎（已有）
├─ VIBE_CODING_开发文档.md  # 本文档（也是给 AI 的上下文）
├─ CLAUDE.md                # 项目约定（AI 协作守则，参考 imsyy 的做法）
├─ src-tauri/
│  ├─ src/{main.rs,command.rs,process.rs,parser.rs,queue.rs,config.rs}
│  └─ tauri.conf.json       # 图标/窗口/插件/bundle
├─ src/                     # React 前端
│  ├─ components/{UrlInput,PreviewCard,OptionsPanel,TaskTable,LogConsole,CommandBar,SettingsPage}
│  ├─ store.ts              # Zustand：tasks / settings / log
│  ├─ lib/{ytdlp.ts(前端事件订阅), format.ts(格式列表筛选排序)}
│  └─ presets.ts            # 预设定义（与 dsymbol 的 config.toml 同思想）
└─ public/icons/…
```

---

## 7. 关键实现要点

### 7.1 参数构造器（command.rs）——纯函数，必须可单测
`build_args(task: TaskConfig, settings: GlobalSettings) -> Vec<String>`，规则：
1. 固定头：`--no-color --newline --windows-filenames --progress-delta 0.3`
   + `--progress-template "download:YDLP|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.speed)s|%(progress.eta)s|%(info.title)s"`
   + `--progress-template "postprocess:YDLP|postprocess|%(progress.status)s"`（可选）
   + `-O "post_process:FILE|%(filepath)s"`（完成时拿最终路径）
2. 预设 → `-f` 表达式（见 §8）；音频预设 → `-x --audio-format X --audio-quality Y`。
3. 全局项：`--paths <outDir>`、`-o <template>`、`-N`、`-r`、`-R 10`、`--retries infinite`。
4. 条件项：cookies / proxy / 字幕 / 嵌入 / SponsorBlock / 播放列表选择。
5. 最后才是 URL。参数顺序：yt-dlp 选项与 URL 混排均可，统一放前。

### 7.2 进程池（process.rs）
- `tokio::process::Command` + `creationflags(CREATE_NO_WINDOW)`（Windows 不闪黑框，必须！）。
- 环境变量注入 `PYTHONUTF8=1`；`stdout(Stdio::piped())`，`stderr` 合并或单独管道。
- `tokio::io::BufReader::lines()` 逐行读；行 → parser → `app.emit()`。
- 并发上限（默认 2 个任务并行）+ 每任务 `-N 4` 分片线程。
- kill：Windows 用 `taskkill /F /T /PID`；任务句柄存 `HashMap<taskId, Child>`。

### 7.3 队列持久化（queue.rs）
- `tauri-plugin-store` 存 JSON；每次状态变更即写（防崩溃丢任务）。
- 启动恢复：`downloading` 态的任务 → 标记 `paused`，用户点继续（自动带 `--continue`）。

### 7.4 预览卡片（PreviewCard）
- 粘贴 URL 防抖 600ms → `invoke("get_info", url)` → Rust 跑 `-J --no-playlist`（超时 15s 可取消）→ 解析 JSON → 卡片显示。
- 检测到 `playlist` 字段 → 播放列表模式：列表展示条目（标题/时长/封面缩略），支持全选/勾选 → `--playlist-items "1,3,5"`。
- 格式选择器：把 `formats[]` 按 `resolution` 去重分组，标注 `filesize/fps/dynamic_range(HDR)`；默认选中预设对应档。

### 7.5 命令预览条（CommandBar）
- 前端调 `invoke("build_command", task, settings)` 返回 `String`（args join，带引号）实时刷新；一键复制。调试神器 + 用户学习入口。

### 7.6 错误模式映射（parser.rs，stderr 尾部匹配）
| 特征 | 友好提示 |
|---|---|
| `Sign in to confirm your age` / `age confirmation` | 需要年龄验证，建议导入浏览器 Cookies |
| `Sign in to view full functionality` / `403` + `Bilibili` | 未登录：高清需要 Cookies，请在设置导入 |
| `Unable to extract` / `No video formats` | 该链接无法解析或需要登录/地区限制 |
| `ffmpeg` + `not found` | 未检测到 ffmpeg，合并/音频功能不可用 |
| `Requested format is not available` | 所选清晰度不存在，已可用档位见预览 |
| `Unable to load cookies` / browser busy | 请关闭目标浏览器后重试 |
| `socket.timeout` / `timed out` | 网络超时，可重试或配置代理 |
兜底：显示 stderr 最后 3 行原文 + “查看详情”展开全日志。

### 7.7 其他细节
- **日志控制台**：所有非进度行按任务着色滚动显示，支持清空/导出 `debug.log`（抄 dsymbol）。
- **打开文件夹**：done 后按钮 → `tauri-plugin-shell` 执行 `explorer /select,"path"`。
- **拖拽/剪贴板**：窗口支持拖入 URL 文本；启动时读剪贴板自动填入输入框（B站复制网页地址即可下）。
- **i18n**：zh-CN 默认 + en（react-i18next）。

---

## 8. 预设与命令映射表（GUI 控件 → CLI）

**预设（presets.ts，思想来自 dsymbol/yt-dlp-gui 的 config.toml）：**

| 预设名 | 生成参数 |
|---|---|
| 最佳画质 | `-f "bv*+ba/b"` |
| MP4 优先（免转封装） | `-f "bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/bv*+ba/b"` |
| ≤1080p | `-f "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b"` |
| ≤720p | `-f "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b"` |
| 仅音频 MP3 | `-x --audio-format mp3 --audio-quality 192K` |
| 仅音频 M4A | `-x --audio-format m4a` |
| 视频+字幕+封面 | 任意 -f + `--write-subs --sub-langs "zh.*,en.*" --embed-subs --embed-thumbnail --embed-metadata` |

**通用控件映射：**

| 控件 | CLI |
|---|---|
| 下载目录 | `--paths <dir>` |
| 文件名模板 | `-o "%(title)s [%(id)s].%(ext)s"`（默认，可改） |
| 并发分片 | `-N 4` |
| 限速 | `-r 1M` |
| 重试 | `-R 10` / `--retries infinite` |
| Cookies(浏览器) | `--cookies-from-browser edge` |
| Cookies(文件) | `--cookies <path>` |
| 代理 | `--proxy http://127.0.0.1:7890`（支持 socks5） |
| 仅单个视频 | `--no-playlist` |
| 播放列表条目 | `--playlist-items "1-5,10"` |
| SponsorBlock | `--sponsorblock-remove all` |
| 合并格式 | `--merge-output-format mp4` |
| 续传（内部） | `--continue` |

**平台经验（写进设置页提示/帮助文档）：**
- **B站**：1080P/帧享需登录 Cookies（Edge/Chrome）；DASH 格式需 ffmpeg 合并；合集用 `--playlist-items` 选集。
- **YouTube**：遇 `Sign in to confirm you're not a bot` → 导入 Cookies；高级可用 `--extractor-args "youtube:player_client=default,web_embedded"` 绕过（放高级选项）。
- **抖音/TikTok**：网页版链接直接可下；App 分享链接需转换为网页版（`v.douyin.com` 短链可直接解析）。
- **直播**：yt-dlp 原生支持，录播需 `--live-from-start`；直播中任务为长任务，允许无限时长。

---

## 9. Vibe Coding 里程碑（每个 M = 一个 AI 会话/迭代）

> 工作方式：每轮把本文档 + 当前代码库丢给 AI，只让它做当前里程碑，做完跑验收清单，commit 后再开下一轮。

### M0 脚手架（验收：窗口能显示引擎版本）
- `pnpm create tauri-app`（React-TS 模板）+ Tailwind + shadcn/ui + Zustand。
- Rust 侧：读 `code/yt-dlp.exe --version`（相对路径可配置）→ 前端展示；`ffmpeg -version` 检测结果展示；窗口深色主题。
- 写 `CLAUDE.md`（项目约定：代码风格、文件职责、禁止 shell 拼接、进度行协议常量集中定义）。
- 验收：`pnpm tauri dev` 启动无黑框，顶部显示 `引擎 2026.08.19 ✅ / ffmpeg 8.1 ✅`。

### M1 预览（验收：粘贴 B站/YouTube 链接 2s 内出卡片）
- `get_info` 命令：`-J --no-playlist`，15s 超时，解析 JSON；播放列表检测。
- PreviewCard：封面/标题/UP 主/时长/格式分组选择器（按 resolution 去重，标 HDR/fps/大小）。
- 验收：B 站单视频 + YouTube 视频 + 一个播放列表各测一次；非法 URL 有友好报错。

### M2 单任务下载（验收：进度条/速度/ETA 实时刷新，可取消）
- command.rs（纯函数+单测）+ process.rs（spawn/NO_WINDOW/行读/kill 进程树）+ parser.rs（YDLP 协议）。
- 前端 TaskTable 单行：进度条、速度、ETA、取消按钮、完成后"打开文件夹"。
- 验收：下载一个 B 站视频 + 一个 MP3 提取（验证 ffmpeg 链路）；下载中取消 → 不留残留进程；杀进程后重启 → 任务可 `--continue`。

### M3 选项面板（验收：每个控件改完命令预览条同步变化）
- OptionsPanel：预设下拉 + 自定义 -f + 音频质量 + 目录 + 文件名模板 + 分片/限速 + 字幕 + 嵌入 + SponsorBlock + 代理 + Cookies。
- CommandBar 实时命令预览（§7.5）；设置持久化（tauri-plugin-store）。
- 验收：勾选各种组合，复制命令到终端手动执行，行为与 GUI 一致。

### M4 队列（验收：3 任务并发下载互不阻塞，崩溃不丢任务）
- 并发上限 2（可配）；队列 JSON 持久化 + 启动恢复；重试/删除/全部暂停；任务日志抽屉。
- 验收：同时下 3 个（2 跑 1 排队）；关程序重开，paused 任务可续传。

### M5 播放列表/合集（验收：B站合集选 3 集下 3 集）
- 预览列表勾选 → `--playlist-items`；条目级状态展示；整列表下载。
- 验收：B 站合集 + YouTube playlist 各测。

### M6 工具盒（验收：字幕下载+双语、封面下载可用）
- 字幕工具（SRT/VTT/ASS/LRC、语言多选、双语合并 `--convert-subs`/ASS 双轨）；封面下载器（`--write-thumbnail` 多分辨率）；元数据导出（`-J` 存 JSON）。

### M7 打磨（验收：新用户 3 步完成第一次下载）
- 设置页（引擎路径/更新检查 `-U`/主题/语言）；首次引导；托盘最小化；错误提示全部走 §7.6 映射；i18n；快捷键；剪贴板自动填充。

### M8 发布（验收：别人电脑双击安装即可用）
- 图标/NSIS 安装包（或便携 ZIP，抄 dsymbol 发布形态）；README；可选捆绑 ffmpeg；代码签名（有证书的话）；GitHub Releases 自动构建（tauri-action）。

**每轮给 AI 的提示词模板：**
```
你是本项目的全栈开发。先读 VIBE_CODING_开发文档.md 与 CLAUDE.md，再看当前代码。
本轮只做 M<x>：……（贴该节内容）。约束：
1) 不改动已完成里程碑的行为；2) 参数构造必须纯函数+单测；3) 禁止 shell 字符串拼接；
4) 进度协议常量只定义在 parser.rs 一处；5) 完成后给出运行验收步骤。
```

---

## 10. 风险与坑（提前告知 AI 和未来的自己）

1. **Windows 黑框**：`CREATE_NO_WINDOW` 必须加，否则每次 spawn 闪控制台。
2. **编码**：子进程 `PYTHONUTF8=1` + 读管道 `utf-8`（Rust 默认就是）；输出到日志前清洗 ANSI（双保险）。
3. **`total_bytes` 缺失**（某些站点）→ 进度条转"无确定进度"样式，别 NaN。
4. **杀进程要杀树**：yt-dlp 会再 spawn ffmpeg，只杀父进程会留孤儿 ffmpeg 占磁盘写入。
5. **Cookies 浏览器占用**：Edge/Chrome 运行时 cookie 库被锁 → 捕获报错映射成"请关闭浏览器"。
6. **URL 注入**：参数数组传递；日志展示时对 URL 做 HTML 转义（防 XSS，React 默认安全但 markdown 渲染日志时注意）。
7. **yt-dlp 更新破坏性**：模板字段名（如 `total_bytes_estimate`）以 `yt-dlp -h` 当前版本为准；M8 加"引擎版本不兼容"检测（`--version` 解析失败 → 提示更新）。
8. **大播放列表 `-J` 慢**：条目多时 JSON 巨大 → 预览超时 30s + 分块展示。
9. **磁盘/路径**：下载目录不存在时先创建；路径含空格/中文全程用参数数组不拼接。
10. **法律/合规**：README 声明仅用于下载有版权或自有内容（参考各开源项目措辞）。

---

## 11. 参考清单

- yt-dlp 仓库/文档：https://github.com/yt-dlp/yt-dlp （`-h` 全量选项；`--print`/`--progress-template` 章节）
- imsyy/yt-dlp-gui：https://github.com/imsyy/yt-dlp-gui （体验对标；其 `yt-dlp.md`、`CLAUDE.md` 值得通读）
- dsymbol/yt-dlp-gui：https://github.com/dsymbol/yt-dlp-gui （预设 config.toml 设计）
- wshhja/yt-dlp-gui：https://github.com/wshhja/yt-dlp-gui （命令实时预览、Qt6 深色主题）
- Tauri 2 文档：https://v2.tauri.app （Process/Shell/Store 插件）
- shadcn/ui：https://ui.shadcn.com
- yt-dlp 选项速查（社区）：https://www.ditig.com/yt-dlp-cheat-sheet
- ffmpeg 下载（Windows）：https://www.gyan.dev/ffmpeg/builds/



