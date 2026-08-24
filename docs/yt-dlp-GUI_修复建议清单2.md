# yt-dlp-GUI 代码审核成果（仅优化 / 修复项）

本次按当前 `main` 代码继续审核，下面不重复已经完成的上一轮修复，只列目前仍建议处理的问题。

## P0 — 建议立即修复

### 1. 删除任务后，旧子进程可能再次把任务“复活”到 UI

**涉及：**

* `src-tauri/src/tasks.rs`
* `src/store.ts`

`remove_task()` 删除任务时，并没有使当前 `run_generation` 失效。特别是“暂停 → 立即删除”场景，原来的子进程 watcher 仍持有 `Arc<TaskInner>`；进程真正退出后，它还能执行 `apply_child_exit()` → `emit_payload()`。

而前端收到任何 `task_updated` 都会重新插入任务：

```ts
return { tasks: [row, ...s.tasks.filter((t) => t.id !== p.id)] };
```

所以已经删除的任务存在重新出现在列表中的竞态。

**修复：**

* `remove_task()` 无论当前状态如何，都应在 `run_mu` 下递增 `run_generation`；
* 若仍存在 PID/Child，必须执行终止；
* watcher 在 emit 前再次确认 generation；
* 最好增加 `removed/tombstone` 状态作为第二道保护。

增加回归测试：

`Downloading → Pause → Remove → old child exit → task must never reappear`

---

### 2. `queue.json/settings.json` 的“原子写”存在并发覆盖和数据丢失风险

`atomic_write()` 对同一个目标始终使用固定临时文件：

```text
.settings.json.tmp
.queue.json.tmp
```

两个并发写入会同时 truncate/write 同一临时文件；同时，两个不同时间取得的 queue snapshot 也可能按相反顺序完成 rename，最终让**旧状态覆盖新状态**。

任务退出、暂停、取消、启动等都会调用 `persist()`，而且当前 queue persistence 直接忽略错误：

```rust
let _ = crate::fsutil::atomic_write_json(...)
```

因此问题可能完全静默发生。

前端 Settings 的 `saveSeq` 只防止旧请求更新前端状态，并不能阻止旧请求最后把旧数据写回磁盘。

**修复：**

* 每次写入生成唯一 temp 文件；
* `settings.json`、`queue.json` 分别建立单 writer/mutex；
* 使用 revision/version，旧 revision 禁止提交；
* `persist()` 返回 `Result`，禁止吞错误；
* rename 后清理残留 temp；
* 增加 20～100 个并发 writer 压测。

这项建议和“设置原子写”一起重新设计，而不是只改临时文件名。

---

## P1 — 高优先级修复

### 3. `update_engine` 仍可能修改系统/PATH/用户 Override 的 yt-dlp

目前仅 `Bundled` 会复制到 managed 目录：

```text
Bundled → Managed
Override → 原路径
Managed  → 原路径
PATH     → 原路径
```

之后直接对 `target` 执行：

```text
yt-dlp -U
```

所以当 yt-dlp 来自 PATH 或用户指定路径时，应用仍可能修改 Scoop/Homebrew/pip/其他软件管理的二进制，甚至修改用户不希望 GUI 管理的文件。

而 locator 本身明确区分 `Override / Managed / Bundled / Path`。

**建议统一成：**

```text
Bundled  ─┐
PATH     ─┼─> copy → AppLocalData/engines/yt-dlp → -U
Override ─┘
Managed  ────────────────────────────────────────→ -U
```

同时：

* 更新操作加全局 mutex；
* 更新前 backup；
* 更新后执行 `--version`；
* 验证失败自动 rollback；
* 禁止重复点击同时运行两个 `-U`。

---

### 4. Proxy 凭据可能同时泄漏到磁盘和命令预览

`GlobalSettings.proxy` 会直接序列化进 `settings.json`；每个任务又保存完整 `NewTask` 到 `queue.json`。如果用户填写：

```text
http://username:password@proxy.example.com
```

账号密码会成为明文持久化数据。

同时 `buildCommand()` 又会把完整 proxy 参数生成出来，`CommandBar` 直接显示并允许复制。

**修复：**

* UI command preview 与真正 argv 分离；
* 显示时转换为 `http://username:***@host`；
* queue 不重复保存认证信息，只保存 auth-profile 引用；
* Proxy 密码如需长期保存，使用 Windows Credential Manager / macOS Keychain / Linux Secret Service。

---

### 5. Queued 状态任务在 UI 中无法取消或删除

后端其实支持取消/暂停 queued 任务，但 `TaskTable.tsx`：

* `running` 只包括 `starting/downloading/postprocess`；
* 删除按钮只出现在 `done/failed/canceled/paused`。

因此排队中的任务没有任何控制按钮。用户一次加入几十条 URL 后，无法撤销尚未开始的任务。

**修复：**

* `queued` 提供 Cancel / Remove；
* 或提供“暂停队列 / 清空等待队列”；
* backend 保持 `Queued → Canceled/Paused` 状态转换。

---

### 6. Toolbox 专用任务会继承普通下载选项，可能产生组合错误

Toolbox 调用：

```ts
enqueue(url, {
  skipDownload: true,
  writeInfoJson: true,
  ...
})
```

但 `buildTaskFromOptions()` 会先继承当前 Options，包括：

* `writeSubs`
* `embedThumbnail`
* `embedMetadata`
* `sponsorblock`
* custom format/preset 等

然后才覆盖 Toolbox 指定字段。

例如“仅下载元数据”可能同时带上字幕、封面等开关。

**修复：**

不要让 Toolbox 通过普通下载配置派生。

建议后端直接建立：

```text
TaskKind::Subtitles
TaskKind::Thumbnail
TaskKind::Metadata
```

并由 Rust 根据 `TaskKind` 构造允许的参数集合，前端不再拼装这些 flag。

---

### 7. 播放列表使用 Video ID 作为选择主键，会处理错误重复视频

目前：

```ts
selectedItems: string[]
key={it.id}
selected.includes(it.id)
```

播放列表完全可能多次包含同一个视频。

结果：

* React 出现重复 key；
* 勾选一次可能影响多个条目；
* 取消其中一个会把相同 ID 全部取消；
* `playlistItems` 与界面选择结果可能不一致。

**修复：**

使用 playlist index，而不是 video ID：

```text
selectionKey = playlistIndex
```

或者：

```text
{id}:{playlistIndex}
```

最终 `--playlist-items` 也天然应该根据 playlist position 生成。

同时把：

```ts
selected.includes(...)
```

换成 `Set`，避免大型 playlist 中 O(n²) 查找。

---

### 8. Unix 暂停/取消是 SIGTERM 后立即 SIGKILL，没有任何退出宽限期

目前 Unix：

```rust
SIGTERM
SIGKILL
```

两条紧接执行。

实际上 SIGTERM 几乎没有意义，yt-dlp / ffmpeg 没有机会：

* flush 文件；
* 关闭 container；
* 清理临时文件；
* 完成当前磁盘写入。

**修复流程：**

```text
SIGTERM process group
       ↓
等待 1~3 秒 / try_wait
       ↓
仍未退出
       ↓
SIGKILL
```

取消可以更激进，Pause 更应该优先 graceful termination。

---

### 9. 明确配置错误的 ffmpeg 路径会被静默忽略

下载流程：

```rust
let ffmpeg = find_ffmpeg(...).ok();
```

也就是设置里明确填写了一个错误的 ffmpeg 路径，也直接转换成 `None`。

`build_command()` 同样如此。

这会把：

> 用户明确配置的路径错误

转换成：

> 后面 yt-dlp 为什么合并失败？

**修复：**

* 如果 `ffmpeg_path` 是显式 Override 且无效：立即报错；
* 如果根本没配置，则允许 fallback；
* 对需要 ffmpeg 的操作提前 fail-fast；
* error code 使用 `FFMPEG_INVALID_OVERRIDE` / `FFMPEG_MISSING` 区分。

---

### 10. 输出目录创建失败被完全忽略

下载前：

```rust
let _ = std::fs::create_dir_all(&dir);
```

权限不足、非法路径、磁盘问题全部被吞掉。

之后用户只会看到 yt-dlp 的模糊失败。

**修复：**

```rust
create_dir_all(...)
    .map_err(|e| format!("无法创建输出目录 {}: {e}", dir.display()))?;
```

并提前验证：

* 路径存在/可创建；
* 是否可写；
* 是否确实为目录。

---

### 11. Preview 会把完整 yt-dlp JSON 一次性加载进内存

`get_info()` 使用：

```rust
child.wait_with_output()
```

再：

```rust
serde_json::from_str::<Value>()
```

完整载入整个 playlist/channel。

仓库测试目前只覆盖 1000 条，但大型频道可以远超这个规模。

**修复：**

* Preview 默认限制如 500～1000 项；
* 实现分页/Load more；
* 或使用流式解析；
* 对 stdout 设置最大字节限制；
* 不要在首次 preview 自动加载整个频道。

---

### 12. Settings/Engine Update 都缺少真正的并发写保护

Settings 页保存按钮没有 `saving` 状态，Update Engine 也没有 `updating` 状态，因此快速重复点击可以产生并发 backend invoke。

结合前面的文件写入竞态，会放大问题。

**修复：**

* 前端保存/更新期间按钮 disabled；
* backend 仍必须独立加 mutex，不能只依赖 UI；
* Settings 使用单 writer；
* Engine updater 使用单 updater。

---

### 13. Stable Release 没有验证 Git Tag 与应用版本一致

Release 只检查：

```text
package.json
Cargo.toml
tauri.conf.json
```

三者相等，却没有验证：

```text
github.ref_name == "v" + package.version
```

因此完全可以：

```text
代码版本 0.1.0
git tag v0.2.0
```

然后发布名为 `v0.2.0`、内部实际上是 `0.1.0` 的安装包。

**修复：**

tag build 强制：

```text
refs/tags/v0.2.0
          ==
package version 0.2.0
```

不一致直接终止 Release。

---

### 14. Nightly 在新版本构建成功之前就删除旧版本

当前 main push：

```text
Delete previous nightly
        ↓
Windows/Linux/macOS build
        ↓
Publish new nightly
```

如果其中任何 build 失败，新 nightly 没有生成，旧 nightly 又已经被删除。并且 Release 配置了 `cancel-in-progress: true`，连续 push 会进一步放大这个窗口。

**修复：**

调整成：

```text
Build + Test 全部成功
        ↓
创建/更新 nightly
        ↓
上传完整 assets
        ↓
成功后替换旧 assets
```

不要在 build 前删除当前可用版本。

---

### 15. Stable Release 没有完整执行 CI 质量门

Release 中执行了：

* typecheck
* cargo test

但没有执行：

* `pnpm test`
* `cargo fmt --check`
* `cargo clippy -D warnings`

而普通 CI 只针对 PR/main。直接创建 tag 时，Release 自身没有完整质量门。

**修复：**

* 把 CI 做成 reusable workflow；
* Stable release 必须依赖完整 CI；
* Build/Publish 只能发生在完整 check 全绿之后。

---

## P2 — 建议继续优化

### 16. 状态机已经定义，但生产代码没有真正使用

`TaskStatus::can_enter()` 被标成 `dead_code`，目前基本只在测试中检查；真正业务仍到处：

```rust
p.status = ...
```

直接赋值。

建议统一：

```rust
transition(TaskStatus::Paused)?
```

所有状态变化都走唯一入口。

否则状态机只是文档，不是约束。

---

### 17. 子进程完成检测每 30ms 轮询一次

每个下载任务都会启动：

```text
sleep 30ms
try_wait()
```

的 watcher。

最大 8 个任务时会长期产生大量无意义 wake-up，对桌面应用的 CPU/电池没有必要。

**优化：**

* `child.wait().await`；
* 取消通过 channel / cancellation token；
* generation 继续负责防 stale completion。

---

### 18. 顶层 App 订阅整个 Zustand Store，下载进度会造成大量无意义重渲染

当前：

```ts
const { ... } = useAppStore();
```

没有 selector，意味着整个 App 对 store 的任何变化都敏感，包括高频 task progress。

同时 `TaskTable` 每次任务变化都会 map 全列表，每个 `TaskRow` 又执行一次 `find()`。

**优化：**

* App 使用独立 selectors / `useShallow`；
* tasks 改成：

```text
taskOrder: string[]
tasksById: Map/Record
```

* TaskRow 只订阅自身 ID；
* 100+ tasks 后启用 virtualization。

---

### 19. Command Preview 每次输入一个字符都会发 Tauri IPC，而且存在旧响应覆盖新响应

`CommandBar`：

```ts
useEffect(() => refresh(), [options, previewUrl])
```

而 Options 每次键盘输入都会生成新对象。

`refreshCommand()` 又没有类似 Preview 的 token，所以：

```text
request A
request B
B 返回
A 后返回
```

最终 UI 可以显示已经过期的 A。

**修复：**

* debounce 150～300ms；
* commandPreviewSeq；
* 只接受 latest revision。

---

### 20. 多 URL 入队是逐个 await + 每次重写整个 queue

当前：

```ts
for (const url of urls) {
    await startDownload(...)
}
```

每增加一个任务，backend 都会重新 snapshot + persist 整个队列。

1000 个 URL 会产生接近 O(n²) 的序列化/磁盘写入。

**建议新增 backend：**

```text
start_tasks(Vec<NewTask>)
```

一次：

1. validate；
2. insert 全部；
3. persist 一次；
4. pump_queue 一次。

---

### 21. 播放列表部分选择应压缩为 range

现在直接生成：

```text
1,2,3,4,5,6,7,...
```

大型 playlist 部分选择时会制造非常长的 `--playlist-items` 参数。

改成：

```text
1-200,205-390,500
```

避免最终触碰 OS 进程命令行长度限制。

---

### 22. `--windows-filenames` 被所有平台强制使用

`build_args()` 和 preview 参数都无条件加入：

```text
--windows-filenames
```

因此 Linux/macOS 也被迫使用 Windows 文件名限制。

**修复：**
仅 `cfg(windows)` 增加该参数，或者增加用户可选的“跨平台安全文件名”。

---

### 23. 默认 Downloads 路径不应该手工拼 `HOME/USERPROFILE`

目前：

```text
USERPROFILE\Downloads
$HOME/Downloads
```

在：

* OneDrive Folder Redirection；
* 企业域策略；
* 自定义 Downloads；
* macOS/Linux XDG 用户目录

下都可能错误。

**修复：**
统一使用 Tauri/system known-folder API 获取 Downloads 目录。

---

### 24. Settings/Queue 路径解析失败时不应该 fallback 到 `"."`

现在 `app_config_dir()` 获取失败后：

```rust
PathBuf::from(".")
```

并且 `create_dir_all()` 错误也被忽略。

这可能导致设置莫名写进当前工作目录。

**修复：**
路径函数返回 `Result<PathBuf>`；配置目录不可用时显式报错，不要降级到 CWD。

同时，配置 JSON 解析失败也不要直接静默恢复默认值，应：

* 将损坏文件重命名 `.corrupt-*`；
* 恢复默认；
* 给用户一个明确提示。

---

### 25. `bindTaskEvents()` 在真正绑定成功前就将 `eventsBound=true`

如果第一次：

```ts
await listen(...)
```

失败，后续调用会因为：

```ts
if (eventsBound) return;
```

永远不再重试。

**修复：**

```text
成功绑定全部 listener
        ↓
eventsBound = true
```

失败则恢复 `false`。

同时保存 `unlisten` handle，方便窗口/HMR 生命周期清理。

---

### 26. `build_command` 和真实 download 使用的“有效配置”不是同一条代码路径

真正下载会执行：

```text
apply_settings()
→ to_config()
→ build_args()
```

而 Command Preview 直接：

```text
task.to_config()
→ build_args()
```

因此未来增加 global setting 后很容易出现：

> UI 显示的命令 ≠ 实际执行的命令

目前 `merge_format` 就已经存在这种潜在差异。

**修复：**

建立唯一：

```rust
resolve_effective_config(task, global_settings)
```

Preview 和 Download 必须共享。

---

### 27. Command Preview 的 shell quoting 不够可靠

目前自定义 `quote_arg()` 只处理有限字符集。

但项目同时支持：

* Windows CMD；
* PowerShell；
* Bash/zsh。

三者 quoting 规则不同。

**修复：**

* UI 提供 PowerShell / CMD / POSIX 三种复制格式；
* 或明确显示“仅参数预览，不保证可直接粘贴执行”；
* 不要试图用同一个 quote 算法兼容所有 shell。

---

### 28. PR CI 只在 Ubuntu 编译

当前普通 CI 仅：

```yaml
runs-on: ubuntu-22.04
```

而项目有大量：

```rust
#[cfg(windows)]
#[cfg(unix)]
#[cfg(target_os = "macos")]
```

代码。Windows/macOS 错误只能等 merge 后的 Release workflow 才暴露。

**修复：**
PR 至少增加：

```text
Ubuntu    cargo check/test
Windows   cargo check/test
macOS     cargo check
```

完整 installer build 仍可以留给 Release。

---

### 29. Nightly 引擎没有完整可复现性

Stable 使用 `engines.lock.json + sha256`，但 Nightly 直接请求 `/latest/`，没有 hash 校验。

Nightly 可以继续追 latest，但建议构建时生成：

```text
engines-manifest.json
yt-dlp version + sha256
ffmpeg version + sha256
source URL
```

并跟 Release 一同上传。

这样任何 nightly 都能追溯实际内置了什么。

---

### 30. `Force/--force` 参数实际上没有效果

PowerShell 声明：

```powershell
[switch]$Force
```

Shell 脚本也解析：

```bash
FORCE=1
```

但后续根本没有根据 Force 决定是否覆盖，脚本始终重新下载。

**修复二选一：**

* 实现无 `Force` 时已有文件跳过；
* 或彻底删除 `Force` 参数，避免产生错误语义。

---

## 发布层面还建议补两项

仓库目前明确没有 `LICENSE`，但安装包又直接分发 yt-dlp 和 FFmpeg。

建议补：

* 项目自身 `LICENSE`；
* `THIRD_PARTY_NOTICES`；
* bundled yt-dlp / FFmpeg 的来源、版本、许可证说明；
* Windows code signing；
* macOS signing + notarization。

另外目前 Windows 与 Linux/macOS 使用的 FFmpeg 来源和版本并不一致，Windows 是 9.0.1，而 Linux/macOS lock 为另一套 6.1.1 static build。 建议逐步统一版本族，至少建立跨平台 codec/merge 回归测试，避免不同系统实际下载结果不同。

