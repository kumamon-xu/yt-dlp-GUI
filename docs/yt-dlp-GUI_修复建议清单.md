# yt-dlp-GUI 修复建议清单

---

## P0 — 修复 Tauri 打包资源路径与运行时查找路径不一致

### 涉及文件

- `src-tauri/tauri.windows.conf.json`
- `src-tauri/tauri.linux.conf.json`
- `src-tauri/tauri.macos.conf.json`
- `src-tauri/src/lib.rs`
- `.github/workflows/release.yml`

### 修复目标

确保安装后的应用能够稳定定位随安装包一起打包的：

- `yt-dlp`
- `ffmpeg`

不能依赖：

- 当前工作目录
- 用户 `PATH`
- 开发环境中的 `code/` 目录

### 建议实现

优先使用 Tauri Resource 映射，将源文件明确映射到应用资源目录下的固定位置。

Windows：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "bundle": {
    "resources": {
      "../code/yt-dlp.exe": "code/yt-dlp.exe",
      "../code/ffmpeg.exe": "code/ffmpeg.exe"
    }
  }
}
```

Linux / macOS：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "bundle": {
    "resources": {
      "../code/yt-dlp": "code/yt-dlp",
      "../code/ffmpeg": "code/ffmpeg"
    }
  }
}
```

运行时统一按照：

```text
resource_dir/code/yt-dlp
resource_dir/code/ffmpeg
```

进行查找。

### `find_tool()` 建议查找优先级

建议重新明确为：

```text
1. 用户显式配置路径
2. AppData / Application Support 中的可更新引擎
3. Tauri resource_dir/code/
4. 开发环境项目根目录 code/
5. PATH
```

不要再依赖大量：

```text
cwd
exe parent
../code
resources/
```

这种猜测式路径组合。

### 增加安装产物 Smoke Test

Release CI 中增加安装/打包后的引擎存在性验证。

至少验证：

```text
应用资源目录中存在 yt-dlp
应用资源目录中存在 ffmpeg
文件大小 > 合理阈值
文件可执行
yt-dlp --version 成功
ffmpeg -version 成功
```

更理想的是增加一个 Rust/Tauri 内部测试命令：

```text
check_engine()
check_ffmpeg()
```

在打包产物环境下执行一次。

### 验收标准

- Windows 安装版在全新机器上无需配置 PATH 即可识别 `yt-dlp` 和 `ffmpeg`
- Linux AppImage / deb 可以识别内置引擎
- macOS dmg 安装后可以识别内置引擎
- 删除系统 PATH 中的 `yt-dlp` / `ffmpeg` 后仍正常
- Release CI 能自动发现资源路径错误

---

## P1 — 为下载任务增加 Run Generation，彻底修复暂停/恢复竞态

### 涉及文件

- `src-tauri/src/tasks.rs`

### 当前需要解决的问题

同一个逻辑任务可能经历多次真实进程运行：

```text
Run A
暂停
Run B
重试
Run C
```

旧 Run 的异步 `wait()` / `finalize()` 不能继续修改新 Run 的：

- `status`
- `pid`
- `child`
- `error`
- `file_path`
- `speed`
- `progress`

### 建议数据结构

为每个任务增加运行代次：

```rust
struct TaskInner {
    payload: Mutex<TaskPayload>,
    child: Mutex<Option<Child>>,
    pid: AtomicU32,
    run_generation: AtomicU64,
    stderr_tail: Mutex<VecDeque<String>>,
    canceled: AtomicBool,
    args: Mutex<Vec<String>>,
}
```

每次启动真实下载进程时：

```rust
let generation = inner.run_generation.fetch_add(1, Ordering::SeqCst) + 1;
```

所有异步 reader / waiter 都捕获自己的：

```rust
my_generation
```

任何状态更新前都检查：

```rust
if inner.run_generation.load(Ordering::SeqCst) != my_generation {
    return;
}
```

### 必须覆盖的异步路径

以下所有异步操作都必须带 generation ownership：

- `read_stdout`
- `read_stderr`
- `wait`
- `finalize`
- PID 清理
- stderr tail
- 文件路径更新
- progress 更新

### PID 清理规则

禁止旧 Run 直接执行：

```rust
pid.store(0, ...)
```

应该只有当前 generation 才能清理当前 PID。

建议保存：

```rust
current_pid
current_generation
```

只有匹配时才能：

```text
clear pid
clear child
finalize status
```

### 暂停流程

推荐：

```text
Running
  ↓
Pausing
  ↓
kill current generation
  ↓
等待当前 generation 退出
  ↓
Paused
```

如果不希望暂停操作等待进程真正退出，则至少需要：

```text
Paused 状态立即展示
旧 generation 后续 finalize 无权修改状态
```

### 恢复流程

```text
Paused
  ↓
Queued
  ↓
Starting
  ↓
generation + 1
  ↓
Running
```

### 验收测试

新增自动化测试覆盖：

```text
start A
pause A
immediately resume
spawn B
A exits after B has started
A finalize must NOT:
- mark B failed
- clear B pid
- overwrite B progress
- overwrite B error
```

以及：

```text
start A
cancel A
retry immediately
spawn B
A exit callback arrives late
B remains running
```

---

## P1 — 修复队列 `pump_queue()` 的重复领取竞态

### 涉及文件

- `src-tauri/src/tasks.rs`

### 修复目标

确保同一个 `queued` 任务永远只能被一个 `pump_queue()` 调用领取一次。

### 推荐增加状态

新增：

```text
queued
starting
downloading
postprocess
paused
done
failed
canceled
```

### 正确的领取流程

领取任务必须在同一临界区内完成：

```text
lock TaskManager
  ↓
检查 running_count
  ↓
找到 oldest queued
  ↓
立即 queued → starting
  ↓
释放锁
  ↓
spawn_download()
```

禁止：

```text
发现 queued
释放锁
spawn 前任务仍是 queued
```

### spawn 失败

如果启动失败：

```text
starting
  ↓
failed
```

并记录：

```text
engine missing
permission denied
spawn error
```

### 另一种可选方案

增加全局：

```rust
pump_lock: Mutex<()>
```

确保同一时刻只有一个 pump 在运行。

但即使增加 `pump_lock`，仍建议加入 `starting` 状态，使状态机更清晰。

### 验收测试

模拟：

```text
同时 2~5 次调用 pump_queue
max_concurrent_tasks = 2
存在 10 个 queued task
```

必须保证：

```text
最多启动 2 个
每个 task 只 spawn 一次
没有重复 PID
没有重复 yt-dlp 进程
```

---

## P1 — 重构 Release 策略，禁止稳定版本漂移

### 涉及文件

- `.github/workflows/release.yml`
- `README.md`

### 修复目标

稳定版本必须满足：

```text
一个版本号
=
一个 Git commit
=
一组固定构建产物
```

### 禁止继续使用

```bash
gh release delete "v${VERSION}" --cleanup-tag --yes
```

然后重新创建同一个：

```text
v0.1.0
```

### 推荐 Release 模型

#### main 分支

发布：

```text
nightly
```

属性：

```text
prerelease = true
允许覆盖
允许滚动更新
```

#### Git Tag

只有：

```text
git tag vX.Y.Z
git push origin vX.Y.Z
```

才创建：

```text
Stable Release
```

Stable Release：

```text
永不自动删除
永不移动 tag
永不重新构建覆盖
```

### 推荐触发关系

```yaml
push main:
  -> CI
  -> optional nightly

push tag v*:
  -> full release matrix
  -> stable release
```

### 版本一致性

以下版本应同步：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

建议增加 CI：

```text
verify-version-consistency
```

任何不一致直接失败。

### 验收标准

- `v0.2.0` 发布后永久固定
- 后续 main 提交不会修改 `v0.2.0`
- nightly 可以随 main 更新
- Release 页面可明确区分 Stable / Nightly

---

## P1 — 固定 yt-dlp / ffmpeg 构建依赖版本并校验 Hash

### 涉及文件

- `scripts/fetch-engines.ps1`
- `scripts/fetch-engines.sh`
- `.github/workflows/release.yml`

### 修复目标

同一个应用版本必须始终获得相同的：

- yt-dlp
- ffmpeg

### 当前不建议继续使用

```text
releases/latest
latest/download
ffmpeg-master-latest
```

作为稳定 Release 构建来源。

### 推荐新增

```text
engines.lock.json
```

示例：

```json
{
  "yt-dlp": {
    "version": "2026.xx.xx",
    "windows-x64": {
      "url": "...",
      "sha256": "..."
    },
    "linux-x64": {
      "url": "...",
      "sha256": "..."
    },
    "linux-arm64": {
      "url": "...",
      "sha256": "..."
    },
    "macos": {
      "url": "...",
      "sha256": "..."
    }
  },
  "ffmpeg": {
    "version": "...",
    "windows-x64": {
      "url": "...",
      "sha256": "..."
    }
  }
}
```

### 构建流程

```text
读取 engines.lock.json
  ↓
下载固定 URL
  ↓
计算 SHA256
  ↓
与 lock 文件比较
  ↓
一致才继续 build
```

### Nightly 可选行为

nightly 可以允许：

```text
latest
```

但 Stable Release 必须使用 lock。

### 验收标准

相同 Git Tag 多次构建时：

```text
yt-dlp hash 一致
ffmpeg hash 一致
应用源码一致
```

---

## P2 — 修复 Settings 写盘成功但内存状态未更新的问题

### 涉及文件

- `src-tauri/src/config.rs`

### 修复目标

禁止出现：

```text
settings.json = 新配置
AppState = 旧配置
```

### 当前需要替换的逻辑

不要使用：

```rust
try_lock()
```

去更新运行时配置。

改为可靠：

```rust
let mut state = app.state::<AppState>().settings.lock()
    .map_err(|_| "settings lock poisoned")?;

*state = settings;
```

或者迁移到：

```rust
tokio::sync::RwLock
```

### 推荐保存顺序

```text
验证 settings
  ↓
原子写盘
  ↓
更新 AppState
  ↓
返回成功
```

如果任何一步失败：

```text
返回 Err
```

前端不能显示保存成功。

---

## P2 — Settings 文件改成原子保存

### 涉及文件

- `src-tauri/src/config.rs`

### 推荐流程

不要直接：

```rust
std::fs::write(settings.json)
```

建议：

```text
settings.json.tmp
  ↓
write
  ↓
flush
  ↓
rename
  ↓
settings.json
```

Windows 下需要处理目标文件已存在情况。

### 可选方案

使用成熟的 atomic write crate，或者自行封装：

```rust
atomic_write_json(path, data)
```

队列文件 `queue.json` 也建议使用相同机制。

### 验收标准

模拟程序在写文件过程中崩溃：

```text
settings.json
queue.json
```

至少有一个完整旧版本或完整新版本，不能留下半截 JSON。

---

## P2 — SettingsPage 改为 Draft + Save 或统一 Debounce

### 涉及文件

- `src/components/SettingsPage.tsx`
- `src/store.ts`

### 推荐方案 A：Draft + Save

打开页面时：

```text
settings
  ↓
local draft
```

所有 input：

```text
只修改 draft
```

点击：

```text
保存
```

才调用一次：

```text
saveSettings()
```

推荐加入：

```text
保存
取消
恢复默认
```

### 推荐方案 B：Debounce

如果希望自动保存：

```text
500ms debounce
```

并且必须：

```text
只允许最后一次请求生效
```

可使用：

```text
save sequence / token
```

防止：

```text
旧请求晚返回
覆盖新状态
```

### 验收标准

连续快速输入 50 个字符：

```text
不能产生 50 次磁盘写入
最终内存配置与 settings.json 必须一致
```

---

## P2 — `update_engine()` 必须检查退出状态

### 涉及文件

- `src-tauri/src/lib.rs`
- `src/components/SettingsPage.tsx`

### 修复建议

执行：

```rust
yt-dlp -U
```

后检查：

```rust
if !out.status.success() {
    return Err(friendly_error(...));
}
```

不能只返回 stdout/stderr。

### 返回结构建议

不要只返回 String。

建议：

```rust
struct EngineUpdateResult {
    updated: bool,
    old_version: Option<String>,
    new_version: Option<String>,
    message: String,
}
```

### 验收标准

以下情况必须正确显示失败：

- 网络断开
- 安装目录不可写
- yt-dlp 自更新不支持
- 文件被占用
- 更新服务器异常

---

## P2 — 不要直接更新安装包内的 yt-dlp，增加独立 Engine 目录

### 涉及文件

- `src-tauri/src/lib.rs`
- `src-tauri/src/config.rs`
- `src-tauri/src/tasks.rs`
- `src-tauri/src/info.rs`

### 推荐运行时目录

Windows：

```text
%LOCALAPPDATA%/<app>/engines/
```

macOS：

```text
~/Library/Application Support/<app>/engines/
```

Linux：

```text
~/.local/share/<app>/engines/
```

### 推荐查找顺序

```text
用户指定 engine_path
  ↓
AppData/engines/yt-dlp
  ↓
bundled resource/code/yt-dlp
  ↓
PATH
```

### 更新方式

```text
下载新 yt-dlp 到临时文件
  ↓
校验 hash / version
  ↓
原子替换 AppData engine
```

Bundled engine 仅作为：

```text
factory fallback
```

### 原因

避免：

- Windows Program Files 写权限问题
- macOS App Bundle 被修改后签名失效
- AppImage 只读
- 更新失败破坏内置 engine

---

## P2 — 增加 Engine 版本与来源状态

### 推荐新增状态

```ts
interface ToolStatus {
  available: boolean;
  path: string | null;
  version: string | null;
  source: "override" | "managed" | "bundled" | "path" | null;
  error: string | null;
}
```

UI 显示：

```text
yt-dlp 2026.xx.xx · Bundled
ffmpeg 8.x · Managed
```

用于快速排查：

```text
实际用了哪个 yt-dlp
```

---

## P2 — Playlist Preview 改为 Flat / Lazy 模式

### 涉及文件

- `src-tauri/src/info.rs`
- `src/components/PreviewCard.tsx`

### 修复目标

避免大型 playlist / channel：

```text
yt-dlp -J
```

一次获取全部 formats 和完整 entry 信息。

### 推荐两阶段解析

#### 第一阶段

Playlist / Channel：

```bash
yt-dlp --flat-playlist -J URL
```

只获取：

```text
id
title
duration
thumbnail
url
index
```

#### 第二阶段

用户点击具体条目时：

```text
get_info(item_url)
```

再获取：

```text
formats
codec
filesize
resolution
```

### 可进一步增加分页

UI：

```text
前 100 条
加载更多
```

避免 5000 条内容全部进入 React DOM。

### 验收标准

至少验证：

- 100 条 playlist
- 1000 条 playlist
- YouTube Channel
- Bilibili 合集

Preview 不应因为固定 30 秒超时而普遍失败。

---

## P2 — Preview Timeout 改为按请求类型配置

### 涉及文件

- `src-tauri/src/info.rs`

### 建议

单视频：

```text
30 秒
```

Playlist flat：

```text
45~60 秒
```

不要简单把所有请求统一改成很大的 timeout。

### 可选增加

```rust
PreviewMode {
    Single,
    PlaylistFlat,
}
```

---

## P2 — 修复默认 Preset 的完整持久化链路

### 涉及文件

- `src/components/SettingsPage.tsx`
- `src/store.ts`
- `src-tauri/src/config.rs`

### 修复目标

用户可以明确设置：

```text
默认下载格式：
- MP4
- Best
- 1080p
- 720p
- MP3
- M4A
```

Settings UI 增加：

```text
Default Preset
```

Quick Download 明确使用：

```text
settings.defaultPreset
```

OptionsPanel 中临时修改 preset 不应自动改变全局默认值，除非产品明确如此设计。

---

## P2 — 校验用户输入的 yt-dlp 参数

### 涉及文件

- `src-tauri/src/command.rs`
- `src/components/OptionsPanel.tsx`

### 需要校验

#### `concurrent_fragments`

建议限制：

```text
1 ~ 32
```

避免：

```text
999999
```

#### `limit_rate`

验证 yt-dlp 可接受格式，例如：

```text
500K
2M
10M
```

#### `proxy`

至少校验：

```text
http://
https://
socks4://
socks5://
socks5h://
```

#### `playlist_items`

校验为：

```text
1,2,5-10
```

等合法格式。

#### `custom_format`

可以允许高级用户自由输入，但应：

```text
trim
非空校验
错误提示
```

### 原则

前端校验用于 UX。

Rust 后端仍必须独立校验，不能信任前端。

---

## P2 — 队列持久化增加 Schema Version

### 涉及文件

- `src-tauri/src/tasks.rs`
- `src-tauri/src/config.rs`

### 推荐格式

当前不要只保存：

```json
[
  { "payload": {} }
]
```

改成：

```json
{
  "schemaVersion": 1,
  "tasks": []
}
```

### 原因

未来修改：

```text
TaskPayload
NewTask
status
字段命名
```

后仍可以迁移旧配置。

### 启动加载

```text
schemaVersion = 1
  ↓
正常解析

未来 schemaVersion = 2
  ↓
migration v1 -> v2
```

---

## P2 — 恢复队列时明确处理非终态任务

### 涉及文件

- `src-tauri/src/tasks.rs`

### 推荐恢复规则

应用异常退出前：

```text
starting
downloading
postprocess
pausing
```

重新启动后统一：

```text
paused
```

并：

```text
speed = 0
pid = 0
child = None
```

### 对 postprocess 特别处理

如果文件已经下载完成但 ffmpeg 后处理未完成：

```text
resume
```

不一定能够恢复真正的后处理阶段。

应测试 yt-dlp 的行为，并明确：

```text
重新执行整个任务
```

还是：

```text
继续已有临时文件
```

---

## P2 — 增加进程启动级别的测试

### 涉及文件

- `src-tauri/tests/`
- `.github/workflows/ci.yml`

### 建议新增 Fake yt-dlp

测试中不要调用真实网络。

创建 fake executable，支持：

```text
--version
模拟 progress
延迟退出
stderr
exit 0
exit 1
生成 FILE_PREFIX
```

用来测试：

- queue
- pause
- resume
- cancel
- retry
- process race
- progress parser integration
- stderr handling

### 重点测试

```text
pause + immediate resume
cancel + immediate retry
remove while running
multiple pump calls
max concurrency
late process exit
spawn failure
```

---

## P2 — 增加前端 Store 单元测试

### 涉及文件

建议新增：

```text
src/store.test.ts
src/lib/format.test.ts
src/lib/ytdlp.test.ts
```

### 推荐使用

```text
Vitest
```

### 至少测试

- `splitUrls`
- `formatOptions`
- `customFormatFromSelection`
- task event merge
- preview token 防旧请求覆盖
- settings debounce / save sequence
- retry 使用原 request
- quick download 使用 default preset

### CI 增加

```bash
pnpm test
```

---

## P2 — 增加 CI Lint / Format

### Rust

增加：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

### TypeScript / React

增加：

```text
ESLint
```

至少覆盖：

```text
React hooks
unused imports
Promise handling
TypeScript correctness
```

### CI 推荐顺序

```text
pnpm install
pnpm exec tsc --noEmit
pnpm lint
pnpm test

cargo fmt --check
cargo clippy
cargo test
```

---

## P2 — Release 增加平台级最小 Smoke Test

### Windows

构建后至少运行：

```text
yt_dlp_gui.exe 可启动
```

以及后端测试：

```text
find_engine
find_ffmpeg
```

### Linux

至少测试：

```text
AppImage 解包后 resource 文件存在
```

如果 CI 环境支持 xvfb，可进一步：

```text
xvfb-run application
```

### macOS

验证：

```text
.app/Contents/Resources/code/
```

资源完整，并确保可执行权限存在。

---

## P2 — Engine 下载脚本增加供应链校验

### 涉及文件

- `scripts/fetch-engines.ps1`
- `scripts/fetch-engines.sh`

### 当前仅检查

```text
文件 > 1MB
```

不足以验证安全性。

### 推荐增加

```text
SHA256
```

固定 Release 使用已知 hash。

下载后：

```text
hash mismatch
  ↓
立即失败
```

### 可增加

```text
yt-dlp --version
ffmpeg -version
```

作为二次验证。

---

## P2 — 修复 macOS / Linux 可执行权限的长期处理

### 涉及文件

- `src-tauri/src/lib.rs`
- `scripts/fetch-engines.sh`

当前 `is_tool_file()` 会尝试：

```text
chmod +x
```

这不应成为生产安装后的正常修复机制。

### 建议

构建阶段就保证：

```text
0755
```

运行时只检查：

```text
is_file
is_executable
```

如果不可执行：

```text
返回明确错误
```

不要静默修改 App Bundle。

---

## P2 — 收紧 Tauri CSP

### 涉及文件

- `src-tauri/tauri.conf.json`

当前：

```text
script-src 'self' 'unsafe-inline'
```

建议确认 Vite/Tauri 生产构建是否确实需要：

```text
unsafe-inline
```

如果不需要应移除。

### 推荐目标

尽量：

```text
default-src 'self'
script-src 'self'
style-src 'self' 'unsafe-inline'
img-src 'self' https: data:
```

缩小 WebView 攻击面。

---

## P2 — Tauri Capability 权限最小化

### 涉及文件

- `src-tauri/capabilities/default.json`

当前启用：

```text
opener:default
dialog:default
```

建议确认实际使用的 opener 权限。

如果并未由前端直接调用：

```text
移除 opener
```

只保留真正需要的 capability。

---

## P3 — `friendly_error()` 避免过度归因

### 涉及文件

- `src-tauri/src/parser.rs`

目前部分规则会把：

```text
404
not found
```

统一解释为：

```text
链接不存在或已删除
```

但可能实际是：

- segment 404
- CDN 404
- API 404
- FFmpeg dependency missing

### 建议

错误模型拆成：

```rust
struct AppError {
    code: String,
    title: String,
    detail: String,
    raw_tail: Option<String>,
}
```

例如：

```text
NETWORK_TIMEOUT
AUTH_REQUIRED
FORMAT_UNAVAILABLE
FFMPEG_MISSING
PROCESS_FAILED
RESOURCE_NOT_FOUND
UNKNOWN
```

UI 再负责中英文显示。

---

## P3 — 将任务状态从 String 改为 Rust Enum

### 涉及文件

- `src-tauri/src/tasks.rs`

当前使用：

```rust
status: String
```

建议：

```rust
enum TaskStatus {
    Queued,
    Starting,
    Downloading,
    PostProcess,
    Pausing,
    Paused,
    Canceling,
    Done,
    Failed,
    Canceled,
}
```

配合：

```rust
#[serde(rename_all = "camelCase")]
```

### 收益

避免：

```text
"postprocess"
"post_process"
"post-processing"
```

这种字符串错误。

同时可以集中实现：

```rust
is_running()
is_terminal()
can_pause()
can_resume()
```

---

## P3 — 为 Task 状态转换增加统一入口

### 建议

不要在不同函数中直接：

```rust
p.status = "..."
```

建议封装：

```rust
transition(task, TaskEvent)
```

例如：

```rust
enum TaskEvent {
    SpawnRequested,
    Spawned,
    Progress,
    PostProcessing,
    PauseRequested,
    ProcessExited,
    CancelRequested,
    RetryRequested,
}
```

长期可以避免非法状态：

```text
done -> downloading
canceled -> postprocess
paused -> done
```

---

## P3 — 对日志进行任务级 Ring Buffer

### 涉及文件

- `src/store.ts`
- `src-tauri/src/tasks.rs`

当前前端：

```text
全局最近 400 条
```

建议：

```text
每任务保留最近 100~300 行
```

同时后端只保留：

```text
stderr tail
```

用于最终 error。

UI 切换任务时显示该任务日志，避免多任务日志混杂。

---

## P3 — `open_folder()` 使用路径参数时增加存在性判断

### 涉及文件

- `src-tauri/src/tasks.rs`

调用前：

```text
path exists?
```

如果最终文件被用户移动/删除：

```text
返回“文件不存在”
```

如果只是文件路径不存在，但父目录存在：

```text
允许打开父目录
```

---

## P3 — 下载完成路径需要兼容多个输出文件

### 涉及文件

- `src-tauri/src/parser.rs`
- `src-tauri/src/tasks.rs`

当前：

```text
file_path: Option<String>
```

对于：

- playlist
- subtitles
- thumbnails
- metadata
- 多文件后处理

可能产生多个输出。

建议长期调整为：

```rust
output_files: Vec<String>
```

主视频可额外：

```rust
primary_file: Option<String>
```

---

## P3 — 对 Toolbox 任务使用专用 Task 类型

### 涉及文件

- `src/components/Toolbox.tsx`
- `src-tauri/src/command.rs`

当前：

```text
字幕
缩略图
metadata
```

通过同一个 Download Task 组合参数完成。

建议增加：

```text
TaskKind:
- Video
- Audio
- Subtitles
- Thumbnail
- Metadata
```

这样：

- 状态文案更准确
- 输出路径更容易处理
- 完成状态更容易识别
- 后续扩展封面/评论/章节等工具更清晰

---

## 推荐最终实施顺序

### 第一阶段：必须先修

```text
1. Tauri bundled resource 路径
2. Task generation / run ownership
3. Queue 原子领取 + Starting 状态
4. Stable / Nightly Release 分离
5. 固定 engine 版本 + hash
```

### 第二阶段：稳定性

```text
6. Settings 内存一致性
7. Settings atomic write
8. Settings draft / debounce
9. Engine 独立更新目录
10. update_engine 正确错误处理
11. Queue schema version
12. Process integration tests
```

### 第三阶段：可维护性

```text
13. TaskStatus Rust enum
14. Task 状态机
15. Playlist flat / lazy preview
16. 前端 Vitest
17. cargo clippy / fmt / ESLint
18. Release smoke tests
```

### 第四阶段：产品完善

```text
19. Engine source/version UI
20. 多输出文件支持
21. Toolbox TaskKind
22. 错误码模型
23. CSP / Capability 收紧
24. 日志结构优化
```

---

## 修复完成后的最低验收矩阵

| 场景 | 必须通过 |
|---|---|
| Windows 安装后无 PATH | 能识别内置 yt-dlp / ffmpeg |
| Linux AppImage | 能识别内置 yt-dlp / ffmpeg |
| macOS dmg | 能识别内置 yt-dlp / ffmpeg |
| 暂停后立即恢复 | 不出现旧进程覆盖新任务 |
| 取消后立即重试 | 新任务 PID 不被旧任务清零 |
| 多线程调用 pump | 同一 queued task 只启动一次 |
| 最大并发 = 2 | 永远不超过 2 个真实任务 |
| 设置连续修改 | settings.json 与内存最终一致 |
| 写配置时异常退出 | 配置文件仍为有效 JSON |
| yt-dlp 更新失败 | UI 明确显示失败 |
| 1000 条 Playlist | 可完成快速列表预览 |
| Stable vX.Y.Z | 后续 main push 不会改变 |
| Stable Release | yt-dlp / ffmpeg hash 固定 |
| CI | typecheck + frontend tests + Rust tests + clippy + fmt |
| Release CI | 能验证实际 bundled engine |

