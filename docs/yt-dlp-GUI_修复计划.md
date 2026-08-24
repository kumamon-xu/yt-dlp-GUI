# yt-dlp GUI 修复计划

对照 `docs/yt-dlp-GUI_修复建议清单.md` 与当前源码审核后的实施计划。清单里的问题多数属实；本文件只保留要做的事、落地方式和验收，不重复论证。

状态基准：`main` @ 跨平台 Release 矩阵已通（Windows / Linux x64+arm64 / macOS arm64+x64）。

---

## 审核结论

| 清单项 | 源码结论 | 计划 |
|---|---|---|
| P0 打包资源路径 vs 运行时查找 | **属实，需收紧。** `tauri.*.conf.json` 用资源列表，安装后文件多半在 `resource_dir/yt-dlp[.exe]`，不是 `code/`。`find_tool()` 靠 cwd / exe 旁 / `resources/` / PATH 猜测，PATH 会掩盖打包失败。Unix `is_tool_file()` 会静默 `chmod +x`。Release CI 不检查安装产物里的引擎。 | 纳入第一阶段 |
| P1 Run generation / 暂停恢复竞态 | **属实，高概率 bug。** `spawn_download` 的 waiter 立刻 `take()` Child，然后 `finalize` + **无条件** `pid.store(0)`。pause→立即 resume 时，旧 Run 退出可把新 Run 标成 failed，或清掉新 PID。`finalize` 只在 `status == "paused"` 时跳过写状态，resume 已把状态改成 `queued`/`downloading` 后旧 waiter 仍会覆盖。 | 纳入第一阶段 |
| P1 `pump_queue` 重复领取 | **属实。** 查到 `queued` 后释放锁再 `spawn_download`，期间任务仍是 `queued`。并发 `pump_queue`（spawn 结束、cancel、resume 都会调）可对同一任务 spawn 两次。`running_count` 只计 `downloading`/`postprocess`，spawn 窗口不计。 | 纳入第一阶段 |
| P1 稳定版漂移 | **属实。** `release.yml` 的 `prepare` 在每次 push `main` 时 `gh release delete v${VERSION}`。同一 `v0.1.0` 会被新 commit 覆盖。 | 纳入第一阶段 |
| P1 引擎 latest + 无 hash | **属实。** `fetch-engines.sh/.ps1` 走 `releases/latest` 和 `ffmpeg-master-latest`。同一 tag 两次构建引擎可以不同。 | 纳入第一阶段（Stable）；Nightly 仍可 latest |
| P2 Settings `try_lock` | **属实。** `save_settings` 写盘成功后 `try_lock` 失败仍返回 `Ok`，前端当保存成功，`AppState` 仍是旧值。 | 第二阶段，与原子写、Draft 合并 |
| P2 非原子写盘 | **属实。** `settings.json` / `queue.json` 都是 `std::fs::write`。 | 同上 |
| P2 SettingsPage 每次按键写盘 | **属实。** `patch()` 直接 `persistSettings`。OptionsPanel 有 400ms debounce，Settings 没有。 | 同上，采用 Draft + 保存 |
| P2 `update_engine` 不看退出码 | **属实。** 拼接 stdout/stderr 后一律 `Ok`。 | 第二阶段，与独立引擎目录一起做 |
| P2 更新安装包内 yt-dlp | **属实。** `-U` 打在 `find_engine()` 找到的文件上，安装目录/只读 AppImage/.app 会失败或破坏签名。 | 第二阶段 |
| P2 Playlist 全量 `-J` + 30s | **属实。** `get_info` 统一 30s，播放列表/频道容易超时。 | 第三阶段 |
| P2 默认 Preset 链路 | **部分属实。** `GlobalSettings.defaultPreset` 已有，Settings UI 没有入口；OptionsPanel 改 preset 也不写回默认值。Quick download 用当前 options。 | 第二阶段（小改，跟 Settings Draft 一起） |
| P2 参数校验 | **属实。** `concurrent_fragments` 仅把 0 变成 4，无上限；proxy / rate / playlist_items 无格式校验。 | 第二阶段 |
| P2 队列 schema / 恢复 | 恢复时 running→paused **已有**。缺 `schemaVersion`、缺 `starting`。 | schema 跟队列改动一起；恢复规则保持 paused |
| P2 测试 / lint | **属实。** 无 fake yt-dlp、无 Vitest、CI 无 fmt/clippy/eslint。 | 测试跟 P1 竞态绑定；lint 放第三阶段 |
| P2 Release smoke | **属实。** 只 build + 上传。 | 并进 P0 资源验收 |
| P2 运行时 chmod | **属实。** 不应作为安装后的正常路径。 | 随 P0 改掉 |
| P2 CSP / Capability | **待验证后收紧。** 前端打开文件夹走自定义 `open_folder`，不是 JS `opener`。生产 CSP 的 `script-src 'unsafe-inline'` 需确认 Vite 是否还需要。 | 第四阶段，先验证再改 |
| P3 错误模型 / TaskStatus enum / 状态机 / 日志 / 多文件 / TaskKind | 方向对，不阻塞正确性。 | 第四阶段 |

清单里「推荐最终实施顺序」四阶段合理。下面按可合并的 PR 切开，避免同一文件来回改。

---

## 原则

1. **安装包自带引擎，不靠 PATH。** PATH 只作开发/用户覆盖的最后一档，且 UI 必须标出来源。
2. **一个逻辑任务、同一时刻只有一个真实进程。** 旧进程的异步回调不能改新进程状态。
3. **Stable 不可变。** `vX.Y.Z` = 一个 git commit + 一组锁定的引擎 hash。
4. **写盘成功才返回成功；内存与磁盘一致。**
5. **前端校验只服务 UX，Rust 必须再校验一遍。**
6. **能单测的竞态必须有单测；引擎 I/O 用 fake 二进制，不打真实网站。**

---

## 第一阶段 — 正确性（按顺序做）

目标：安装版一定找得到引擎；暂停/恢复/取消不会串台；稳定版不再被 main 覆盖。

### PR-1 明确打包资源路径 + 查找顺序 + 产物检查

**改**

- `tauri.windows.conf.json` / `tauri.linux.conf.json` / `tauri.macos.conf.json` 资源改为 **map**：

```text
../code/yt-dlp.exe  →  code/yt-dlp.exe     (Windows)
../code/ffmpeg.exe  →  code/ffmpeg.exe
../code/yt-dlp      →  code/yt-dlp         (Unix)
../code/ffmpeg      →  code/ffmpeg
```

- `find_tool()` 只保留：

```text
1. 用户 engine_path / ffmpeg_path（文件存在才用）
2. app_data/engines/（本阶段可先留空位，PR-6 再写更新逻辑）
3. resource_dir/code/<name>
4. 开发：仓库根/CWD 的 code/<name>
5. PATH（仅此时 source=path）
```

删掉 exe parent、`../code`、小写 `resources/` 等猜测。macOS 用 `resource_dir()`，不要再拼 `Contents/Resources`。

- Unix：构建脚本 `chmod 0755`。运行时发现不可执行则 **报错**，禁止静默 chmod（避免改 App Bundle）。
- `ToolStatus` 增加 `source: override | managed | bundled | path | none`（本 PR 先返回 bundled/override/path，managed 等 PR-6）。
- Release job 在 `tauri build` 之后、上传之前：在对应 bundle 目录检查 `code/yt-dlp*`、`code/ffmpeg*` 存在、大小、可执行、`--version`/`-version` 成功。失败则整 job 失败。

Windows 查 NSIS/MSI 解包或 `target/release` 旁 resources；Linux 查 AppImage 的 `squashfs-root` 或 deb data；macOS 查 `*.app/Contents/Resources/code/`。

**验收**

- 全新机器、PATH 里没有 yt-dlp/ffmpeg，安装后状态栏引擎/ffmpeg 为 bundled 且 version 非空。
- CI 故意缺资源文件时 Release 失败。
- `cargo test` 覆盖 `bin_name` 与「不可执行则报错、不 chmod」。

**风险**

- Tauri 2 资源 map 在 Windows/AppImage 上的落盘路径要以一次真实 build 为准，smoke 脚本按实际路径写，不要只抄文档。

---

### PR-2 任务 Run generation + `starting` 原子领取

**改文件：** `src-tauri/src/tasks.rs`（可拆 `src-tauri/src/task_state.rs`）

**状态**

```text
queued → starting → downloading → postprocess → done
                  ↘ paused
                  ↘ failed
         canceled（终态）
```

`pause`：`downloading|postprocess` → 先标 `paused`（或 `pausing` 再 paused），kill **当前 generation**。  
`resume`：仅 `paused` → `queued`，再 pump。  
`cancel`：标 `canceled`，kill 当前 generation；旧 waiter 不得改状态。

**Generation**

- `TaskInner.run_generation: AtomicU64`
- 每次真正 spawn：`gen = fetch_add(1) + 1`，stdout/stderr/wait 闭包捕获 `gen`
- 写 `status` / `pid` / `progress` / `file_path` / `error` / `pid=0` 前：`run_generation == gen`
- waiter **不要**在 spawn 当下就把 `Child` `take()` 走；kill 必须能打到当前 Child。退出后仅当 generation 匹配才 `take` + `finalize`

**pump**

同一把 `TaskManager` 锁内：

```text
running_count（含 starting + downloading + postprocess）< cap
  → 取 oldest queued
  → 立刻 queued 改 starting
  → 放锁
  → spawn
```

spawn 失败：`starting → failed`，再 pump。禁止「仍是 queued 时放锁」。

不单独加 `pump_lock`，`starting` 足够；若实现时仍有并发，再加。

**测试（假进程，无网络）**

`src-tauri/tests/` 或 `tasks.rs` 单测 + 可执行 fake：

1. pause 后立刻 resume：旧 wait 返回不得 failed 新任务、不得 `pid=0`
2. cancel 后立刻 retry：同上
3. 并发多次 `pump_queue`，`max=2`，10 个 queued：最多 2 个活进程，每任务 spawn 一次
4. spawn 失败 → `failed`，队列继续

**验收：** 上表 4 条自动化必过。

---

### PR-3 Release：Nightly vs Stable

**改** `.github/workflows/release.yml`、`README.md`

| 触发 | 产物 | 行为 |
|---|---|---|
| push `main` | tag `nightly`，`prerelease: true` | 允许删旧 nightly 再发 |
| push tag `v*` | tag 即版本 | **禁止** `gh release delete`；tag 不移动 |
| `workflow_dispatch` | 默认当 nightly；输入 `stable=true` 仅用于补打（仍要求已有 tag） | 不覆盖已存在的 stable 资产 |

删掉 `prepare` 里对 `v${VERSION}` 的 delete。

CI 增加 `verify-version-consistency`：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的 `version` 必须相同，否则失败。

**验收**

- 发过 `v0.2.0` 后，再 push main 只更新 nightly，GitHub 上 `v0.2.0` 资产与 tag SHA 不变。
- README 写清：用户下 Latest = 最新 **非 pre** 稳定版；开发包看 nightly。

---

### PR-4 引擎 lockfile + hash（Stable 强制）

新增 `engines.lock.json`（或 `engines.lock.toml`），按平台写死 URL + sha256：

- yt-dlp：windows-x64 / linux-x64 / linux-arm64 / macos-universal（或分 arch）
- ffmpeg：同上

`fetch-engines.sh` / `.ps1`：

- `--lock`（Release tag / 默认 CI）：只下 lock 里的 URL，算 sha256，不一致失败
- `--latest`：仅 nightly 用，仍建议下载后写 version 到日志

Stable job 必须 `--lock`。脚本末尾跑 `yt-dlp --version`、`ffmpeg -version`。

另备 `docs/engines-bump.md` 三步：改 lock → 本地脚本校验 → 提交。不自动跟 latest。

**验收：** 同一 tag 两次 Release，日志里引擎 sha256 相同。

---

## 第二阶段 — 设置与引擎更新

可与第一阶段后期并行，但 **PR-6 依赖 PR-1 的查找顺序空位**。

### PR-5 Settings 一致性（合并清单 3 条）

**磁盘**

- `atomic_write`：写 `*.tmp` → flush → rename 覆盖（Windows 先删目标或用替换 API）
- `settings.json` 与 `queue.json` 共用
- 队列 JSON 改为 `{ "schemaVersion": 1, "tasks": [...] }`；启动时兼容旧数组

**内存**

- `save_settings`：校验 → 原子写盘 → `settings.lock()`（poison 返回 Err）→ 成功
- 禁止 `try_lock` 吞掉更新

**UI**

- SettingsPage：打开时拷 draft；输入只改 draft；**保存 / 取消**
- 增加 Default Preset 下拉，写入 `defaultPreset`
- OptionsPanel 的 preset **不**自动当全局默认
- Quick download / `enqueueUrls` 无显式 preset 时用 `settings.defaultPreset`
- Options 里目录/代理等若仍要自动保存，保留 debounce，并加 save generation，避免旧请求覆盖

**校验（Rust 必做）**

| 字段 | 规则 |
|---|---|
| `concurrent_fragments` | 1..=32 |
| `max_concurrent_tasks` | 1..=8 |
| `limit_rate` | 空或 `^\d+(\.\d+)?[KMG]?$` |
| `proxy` | 空或 `https?://` `socks4://` `socks5://` `socks5h://` |
| `playlist_items` | `^\d+(-\d+)?(,\d+(-\d+)?)*$` |
| `custom_format` | trim，允许空（空则走 preset） |

**验收**

- 设置页连打 50 字符：最多一次（保存时）或 debounce 后一次写盘；内存 = 文件
- 保存时锁被占：返回错误，UI 不显示成功
- 写盘中杀进程：留下完整旧文件或完整新文件

---

### PR-6 用户目录引擎 + 正确的 `-U`

查找顺序（与 PR-1 对齐）：

```text
override → app_data/engines/yt-dlp → resource_dir/code/ → PATH
```

路径：`app_local_data_dir()/engines/`（Win `%LOCALAPPDATA%`，macOS Application Support，Linux `~/.local/share`）。

更新：

1. 下到 temp（优先 lock 里的版本；或官方 release）
2. sha256 / `--version`
3. 原子替换 `engines/yt-dlp`
4. **禁止**写 bundled 文件

`update_engine`：

- 检查 `status.success()`
- 返回 `{ updated, oldVersion, newVersion, message, source }`
- 失败：不可写、网络、不支持自更新、hash 失败 → `Err`，UI 红字

状态栏：`yt-dlp 2026.x.x · Bundled|Managed|PATH|Override`

**验收：** 安装版点更新，只改 AppData；bundled 文件 hash 不变；断网显示失败。

---

## 第三阶段 — 预览与工程化

### PR-7 Playlist / Channel 两阶段预览

- 探测 playlist/channel：`--flat-playlist -J`，超时 45–60s，条目先 id/title/duration/thumbnail
- 单视频或用户点开某集：完整 `-J`，超时 30s
- UI：默认渲染前 100 条 + 加载更多，避免数千节点进 DOM
- 验收：B 站合集、YouTube playlist、频道页不再因 30s 普遍失败（允许网络波动，CI 用录制 JSON fixture，不打真站）

### PR-8 测试与 lint

- Vitest：`splitUrls`、format 选择、preview token、retry 带原 request、defaultPreset
- CI：`pnpm exec tsc --noEmit` → `pnpm test` → `cargo fmt --check` → `clippy -D warnings` → `cargo test`
- fake yt-dlp 已在 PR-2；本 PR 补前端和 clippy 干净度

---

## 第四阶段 — 产品与安全（不阻塞发版）

按需拆 PR，不必一次做完：

1. `TaskStatus` enum + `transition(event)`，非法转换 debug_assert/Err
2. `friendly_error` → `code + title + detail + raw_tail`（404 不再一律「链接不存在」）
3. 每任务 ring buffer 日志；全局 400 条改为按任务 100–300
4. `open_folder`：路径不存在则 Err；仅父目录在则打开父目录
5. `output_files: Vec<String>` + `primary_file`；Toolbox `TaskKind`
6. CSP：生产构建确认无 inline script 后去掉 `script-src 'unsafe-inline'`
7. Capability：确认未用 JS opener 则去掉 `opener:default`（保留 `dialog`）

---

## 建议排期

```text
PR-1 资源路径 + smoke
PR-2 generation + pump starting          } 第一阶段，可 1→2 串行，3/4 并行
PR-3 nightly/stable
PR-4 engines.lock.json

PR-5 Settings 原子写 + Draft + 校验
PR-6 独立引擎目录 + update_engine        } 第二阶段；6 依赖 1

PR-7 flat playlist
PR-8 Vitest / clippy / fmt               } 第三阶段

P3 项按产品需要插入
```

第一阶段做完即可再发一个 **新 tag**（例如 `v0.2.0`）。不要在修 Release 策略前再 bump 同一 `v0.1.0`。

---

## 最低验收矩阵（阶段完成后）

| 场景 | 必须 |
|---|---|
| Win/Linux/macOS 安装，无 PATH | 识别 bundled 引擎 |
| Release CI | 缺引擎或 hash 错则失败 |
| pause 后立刻 resume | 旧 finalize 不动新任务 |
| cancel 后立刻 retry | 新 PID 不被清零 |
| 并发 pump，max=2 | 不超过 2 个真实进程；每任务 spawn 一次 |
| Settings 连改 | 磁盘与内存最终一致；崩溃后 JSON 完整 |
| yt-dlp 更新失败 | UI 失败，bundled 文件完好 |
| 大 playlist | flat 预览可完成 |
| tag `vX.Y.Z` | 之后 main 只更新 nightly |
| 同一 tag 两次构建 | 引擎 sha256 相同 |
| CI | tsc + 单测 + fmt + clippy |

---

## 明确不做（除非另开需求）

- 本计划不包含 Apple 公证、Windows 代码签名（仍可后续加 secrets）
- 不把 nightly 做成 GitHub Latest（Latest 保持最新非 pre 稳定版）
- 不在 CI 里用 xvfb 点完整 GUI 作为 P0；有资源 `--version` 即可
- 不在第一阶段做错误码大重构或 Toolbox TaskKind
