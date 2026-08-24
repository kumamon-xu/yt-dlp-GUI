# yt-dlp GUI 第二轮修复计划

对照 `docs/yt-dlp-GUI_修复建议清单2.md` 与当前 `main`（`0d33c80`）源码审核后的实施计划。上一轮 P0–P3 已合入；本文件只处理第二轮清单，不重复论证已完成项。

状态基准：第一轮修复已推送；Windows / Linux / macOS Release 矩阵与 nightly/stable 分流已存在。

---

## 审核结论

| # | 清单项 | 源码结论 | 计划 |
|---|---|---|---|
| 1 | 删除后旧进程把任务复活到 UI | **属实，P0。** `remove_task` 不 bump `run_generation`。`Paused` 既不是 `is_live()` 也不是 `Queued`，pause→remove **不 kill**。waiter 仍持有 `Arc<TaskInner>`，`apply_child_exit` 返回 true 后无条件 `emit_payload`。前端 `task_updated` 用 ` [row, ...filter]` 把任务插回列表。 | 第一阶段 |
| 2 | 原子写固定 tmp + 并发覆盖 | **属实。** `atomic_write` 固定 `.<name>.tmp`；`persist()` 为 `let _ = atomic_write_json`。Settings `saveSeq` 只挡前端 state，挡不住后到的旧 `save_settings` 写盘。Windows `MoveFileExW` 已比「先删再 rename」安全，但不能解决双 writer。 | 第一阶段，与 #12 的单 writer 合并 |
| 3 | `-U` 可能改 PATH/Override 二进制 | **属实。** `resolve_update_target` 仅 `Bundled` 复制到 managed；`Override`/`Path`/`Managed` 都对原路径 `-U`。 | 第二阶段 |
| 4 | Proxy 凭据进磁盘和命令预览 | **属实。** `GlobalSettings.proxy` 与 `NewTask.proxy` 明文进 `settings.json`/`queue.json`；`CommandBar` 复制完整 argv。 | 第二阶段先脱敏 + 队列不存 userinfo；系统凭据库单列，不阻塞本轮 |
| 5 | Queued 无法取消/删除 | **属实。** 后端 `cancel_task`/`remove_task` 支持 queued；`TaskTable` 的 Cancel 只给 running，Remove 只给 done/failed/canceled/paused。 | 第一阶段（小改，跟 #1 一起） |
| 6 | Toolbox 继承普通下载选项 | **属实。** `enqueueUrls` → `buildTaskFromOptions` 先铺 Options（字幕/封面/sponsor/preset），再覆盖 Toolbox 的几个字段。Rust `TaskKind` 只作展示，`build_args` 不按 kind 收敛。 | 第三阶段 |
| 7 | 播放列表用 video id 当主键 | **属实。** `key={it.id}`、`selectedItems: string[]`、`selected.includes(it.id)`。重复 id 会撞 key、联动勾选。`--playlist-items` 虽按 index 生成，但选择集合按 id。 | 第三阶段 |
| 8 | Unix SIGTERM 后立即 SIGKILL | **属实。** `kill_process_tree` 连续 `kill(pg, SIGTERM)` + `kill(pg, SIGKILL)`。Windows 已是 `taskkill /F`。 | 第三阶段（pause 宽限；cancel 可更短） |
| 9 | 错误 ffmpeg 覆盖路径被 `.ok()` 吞掉 | **属实。** `spawn_download` / `build_command` 都是 `find_ffmpeg(...).ok()`。显式 `ffmpeg_path` 无效时变成「没 ffmpeg」。 | 第一阶段 |
| 10 | `create_dir_all` 忽略错误 | **属实。** `let _ = std::fs::create_dir_all(&dir);` | 第一阶段，跟 #9 同 PR |
| 11 | Preview 整份 `-J` JSON 进内存 | **部分属实。** 已 `--flat-playlist` + UI 先渲染 100 条；但 `wait_with_output` + `serde_json::from_str` 仍吃完整 stdout。测试只覆盖 1000 条。 | 第三阶段：`--playlist-end` / 解析后截断 / stdout 字节上限 |
| 12 | Settings/Update 无并发写保护 | **属实。** 保存按钮无 `saving`；更新无 `updating`；backend 无单 writer mutex（除 settings `Mutex` 本身，且 save 先写盘再锁内存）。 | 并进 #2、#3 |
| 13 | tag 与应用版本未对齐 | **属实。** `verify-version` 只比 `package.json` / `Cargo.toml` / `tauri.conf.json`，不管 `github.ref_name == v$version`。 | 第一阶段 |
| 14 | 先删 nightly 再构建 | **属实。** `prepare` 在矩阵构建前 `gh release delete nightly`。`cancel-in-progress: true` 会拉大空窗。 | 第一阶段 |
| 15 | Stable Release 缺 CI 质量门 | **属实。** Release 有 tsc + cargo test；无 `pnpm test` / `fmt --check` / `clippy -D warnings`。直接打 tag 绕过 PR CI。 | 第一阶段 |
| 16 | `can_enter` 未用于生产赋值 | **属实。** `#[allow(dead_code)]`，业务仍直接 `p.status = ...`。 | 第四阶段 |
| 17 | 30ms `try_wait` 轮询 | **属实。** 每个下载一个 loop。 | 第四阶段：`child.wait().await` + generation |
| 18 | App 订阅整个 store | **属实。** `useAppStore()` 无 selector；`TaskRow` 用 `find`。 | 第四阶段 |
| 19 | Command Preview 无 debounce/token | **属实。** `useEffect` 跟 `options`；`refreshCommand` 无 seq。 | 第四阶段 |
| 20 | 多 URL 逐个 persist | **属实。** `for await startDownload`；每次 `start_task` 都 `persist`。 | 第四阶段：`start_tasks` |
| 21 | playlist-items 不压缩 range | **属实。** `join(",")` 成 `1,2,3,...`。 | 第三阶段，跟 #7 |
| 22 | 全平台 `--windows-filenames` | **属实。** `build_args` 与 preview args 无条件加入。 | 第四阶段 |
| 23 | Downloads 手拼 HOME/USERPROFILE | **属实。** `default_out_dir()`。 | 第四阶段 |
| 24 | 配置目录失败落到 `"."` | **属实。** `app_config_dir().unwrap_or(".")`；JSON 坏了静默默认。 | 第四阶段 |
| 25 | `eventsBound=true` 在 listen 成功前 | **属实。** 先置 true 再 `await listen`。 | 第四阶段 |
| 26 | Preview 与 download 配置路径不一致 | **属实。** download：`apply_settings` → `to_config`；`build_command`：直接 `to_config`。`merge_format` 已能分叉。 | 第三阶段小改，跟 #6 或单独函数 |
| 27 | 单一 quote 算法 | **属实。** `quote_arg` 只包空白和一小段字符。 | 第四阶段：标明「参数预览」；可选分 shell |
| 28 | PR CI 仅 Ubuntu | **属实。** `runs-on: ubuntu-22.04`。 | 第四阶段：Windows/macOS `cargo test`/`check` |
| 29 | Nightly 引擎不可复现 | **属实。** nightly `--latest`，无 hash、无构建清单。 | 第四阶段：上传 `engines-manifest.json` |
| 30 | `Force/--force` 无效 | **属实。** 解析了但从不跳过已有文件。 | 第四阶段：删参数或真正实现 skip |
| 发布 | LICENSE / notices / 签名 / ffmpeg 版本族 | LICENSE 确实没有。ffmpeg lock 为 Win Gyan 9.0.1 vs Unix eugeneware b6.1.1。签名上一轮已列为非目标。 | LICENSE + THIRD_PARTY_NOTICES 本轮做；签名仍不做；ffmpeg 统一单独立项 |

---

## 原则

1. **删掉的任务不能再出现在 UI 或 `queue.json`。** 旧 waiter 的 emit/persist 必须被 generation 或 tombstone 挡住。
2. **同一文件同一时刻只有一个 writer；旧 snapshot 不能覆盖新 snapshot。**
3. **GUI 只更新自己管理的引擎。** PATH / 用户 Override 只读，更新一律落到 `app_local_data/engines/`。
4. **显式配置失败要 fail-fast。** 覆盖路径无效、输出目录建不出来，不能变成「后面 yt-dlp 莫名失败」。
5. **前端校验只服务 UX；列表选择主键必须是 playlist 位置。**
6. **Stable tag 的名字必须等于应用 version；Nightly 先构建成功再换资产。**

---

## 第一阶段 — 正确性（按顺序）

目标：删除不复活；队列/设置写盘不互相踩；queued 能撤；错误 ffmpeg/目录立刻失败；Release 不再先拆 nightly、tag 不再名实不符。

### PR-1 删除任务：generation 作废 + tombstone + 前端忽略

**改** `tasks.rs`、`store.ts`、`TaskTable.tsx`

- `remove_task` 在 `run_mu` 下 `fetch_add` generation，然后 **无论当前状态** 都 `canceled=true` + `kill_inner`（paused 也杀）。
- 从 map/order 移除后记 tombstone（`HashSet<String>` 或 generational slot，重启可丢；`queue.json` 不写已删任务）。
- waiter：`apply_child_exit` 成功后、`emit_payload` 前若 id 已不在 map / 已 tombstone / generation 不匹配，则 **不 emit、不 persist 回该任务**。
- 前端：`task_updated` 若本地已没有该 id 且不是 `startDownload` 刚插入的 stub，则忽略；`remove` 先本地删再 invoke。
- Queued / Paused 显示 Cancel 与 Remove（清单 #5）。

**测试（假进程，无网络）**

1. Downloading → Pause → Remove → 旧 child 退出：`list_tasks` 无此 id，且捕获的 `task_updated` 不得把任务插回（可用 hook/计数，或单测 `should_emit` 辅助函数走真实 `apply_child_exit` + tombstone）。
2. Queued → Remove：不再 spawn。
3. 并发 pump 下删除 starting 任务：最多一个进程，且最终无此任务。

**验收：** pause 后立刻删除，列表不再出现该行；重启后 `queue.json` 也没有。

---

### PR-2 单 writer 写盘 + 唯一 tmp + persist 报错

**改** `fsutil.rs`、`config.rs`、`tasks.rs`、Settings UI

- tmp：`.<name>.<pid>.<uniq>.tmp`，rename 成功后删除残留。
- `settings.json`、`queue.json` 各一把 `Mutex`（或带 revision 的 `AtomicU64`：读 snapshot 时记下 rev，提交时 cas，失败则丢弃旧 snapshot 或重读再写）。
- `persist()` → `Result<(), String>`；调用方至少 `map_err` 打日志，start/save 失败要返回前端。
- Settings 保存中 `saving` disabled；backend `save_settings` 与写盘共用 settings writer mutex（清单 #12 的 settings 半边）。

**测试：** 20+ 线程对同一 path `atomic_write_json` 不同 payload，最终文件是合法 JSON 且等于某一完整写入（不是拼接碎片）；旧 revision 不得覆盖新 revision。

**验收：** 连点保存 / 多任务同时结束，磁盘 JSON 可 parse，且等于内存最后一次成功保存。

---

### PR-3 ffmpeg 显式路径与输出目录 fail-fast

- `ffmpeg_path` 非空且 `locate` 失败 → `Err`（`FFMPEG_INVALID_OVERRIDE`），禁止 `.ok()`。
- 未配置时仍可 fallback；需要 ffmpeg 的 preset（mp3/m4a/merge）找不到则 `FFMPEG_MISSING`。
- `create_dir_all` 失败返回 `无法创建输出目录 …`；可写校验（尝试在目录创建/删除探针文件，或 `OpenOptions`）。
- `build_command` 与 download 共用同一查找函数。

**验收：** 设置里填一个不存在的 ffmpeg，点下载立刻红字，而不是事后 merge 失败。

---

### PR-4 Release：tag=version、先构建再换 nightly、质量门

**改** `.github/workflows/release.yml`、`ci.yml`

- tag 构建：`github.ref_name == "v" + version`，否则 job 失败。
- 删除 `prepare` 里构建前 `gh release delete nightly`。改为矩阵 **全部成功后** 用 `gh release upload`/`tauri-action` 覆盖 nightly 资产（或先发 `nightly-staging` 再 rename；以「旧 nightly 在失败时仍可下载」为准）。
- Release 增加：`pnpm test`、`cargo fmt --check`、`clippy -D warnings`（可 `workflow_call` 复用 CI）。
- `cancel-in-progress`：tag 构建不要取消；main nightly 可保留 cancel，但不得先删资产。

**验收：** 打 `v0.2.0` 而代码仍是 `0.1.0` 时 Release 失败；故意让一个平台构建失败时 GitHub 上仍能下到上一份 nightly。

---

## 第二阶段 — 引擎更新与凭据

### PR-5 只更新 managed 引擎 + 单 updater

```text
Bundled / PATH / Override  → copy 到 app_local_data/engines/ → -U
Managed                    → 直接 -U
Bundled 路径               → 仍禁止写入
```

- 全局 `ENGINE_UPDATE` mutex；UI `updating` disabled。
- 更新前 copy backup；`-U` 后 `--version` 失败则 rollback。
- 状态栏 source 变为 `managed`。

**验收：** PATH 上的 yt-dlp 文件 hash 在「更新引擎」后不变；AppData 里的副本 version 变了。断网/非 0 退出码 → Err + backup 恢复。

---

### PR-6 命令预览与队列脱敏

**做**

- 显示/复制命令时把 URL userinfo 打成 `user:***`（proxy、可能的 cookies 路径保持原样或只显示文件名）。
- `queue.json` 的 `NewTask.proxy`：运行时从 settings 填，或落盘时剥 `user:pass@`。
- 命令预览文案标明「参数预览，不一定可直接粘贴到任意 shell」。

**缓做（本轮不阻塞）**

Windows Credential Manager / Keychain / Secret Service。需要新依赖和卸载迁移，单独立项。

**验收：** settings 里填 `http://alice:secret@127.0.0.1:7890`，CommandBar 看不到 `secret`；打开 `queue.json` 没有 `secret`。真实 argv 仍带完整 proxy（下载必须能用）。

---

## 第三阶段 — 预览、Toolbox、信号

### PR-7 Toolbox 按 TaskKind 收敛参数

- 前端 Toolbox 只传 `{ url, kind }`（外加 cookies/proxy/outDir 等网络与目录，不带字幕/封面/sponsor/preset）。
- Rust `build_args`：`Subtitles` / `Thumbnail` / `Metadata` 只开对应 flag + `--skip-download`，忽略混进来的 embed/sponsor。
- `build_command` 与 download 共用 `resolve_effective_config(task, settings)`（清单 #26）。

**验收：** Options 打开字幕后再点「仅元数据」，命令行没有 `--write-subs`。

---

### PR-8 播放列表用 index 选择 + range 压缩

- `selectedItems: number[]`（playlist 下标，0-based 或 1-based 与 yt-dlp 对齐并写死）。
- React `key={`${index}:${it.id}`}`。
- `Set` 查询；`--playlist-items` 用 `1-3,5,8-10`。
- 后端 `get_info`：`--playlist-end` 默认 1000（或与 UI load-more 对齐）；stdout 超过 N MB 则失败并提示收窄；parse 后截断 entries（清单 #11）。

**验收：** 两条相同 id 的条目可单独勾选；200 个连续选中变成 `1-200`。

---

### PR-9 Unix pause 宽限杀进程

- Pause：`SIGTERM` → 等 1–2s（`try_wait` / `timeout`）→ 仍活则 `SIGKILL`。
- Cancel：0.3–1s 或直接 SIGKILL（文档写清）。
- Windows 保持 `taskkill /F`（没有 SIGTERM 语义）。

**测试：** fake 进程 trap SIGTERM 后 200ms 退出，pause 路径等它自己退，不先 SIGKILL（可用记录 signal 的 fake）。

---

## 第四阶段 — 工程化与体验（不阻塞发版）

按依赖拆 PR，可穿插：

| 项 | 做法 |
|---|---|
| 16 状态机 | 生产路径 `transition(next)?`，非法转换 `debug_assert` + `Err` |
| 17 waiter | `child.wait().await`，kill 仍走 generation |
| 18 渲染 | App/TaskRow 用 selector；`tasksById`；任务很多再虚拟列表 |
| 19 命令预览 | debounce 200ms + seq |
| 20 批量入队 | `start_tasks(Vec<NewTask>)` 一次 persist + 一次 pump |
| 22 文件名 | `--windows-filenames` 仅 Windows，或设置项 |
| 23 Downloads | 用 Tauri `path().download_dir()` |
| 24 配置目录 | `Result`；坏 JSON 改名为 `settings.corrupt-<ts>.json` 并提示 |
| 25 事件绑定 | listen 全成功再 `eventsBound=true`；保存 unlisten |
| 27 quoting | UI 标明预览；可选 PS/CMD/POSIX 三种复制 |
| 28 PR CI | `windows-latest` cargo test；`macos-latest` cargo check |
| 29 Nightly 清单 | 构建结束上传 `engines-manifest.json`（url、sha256、`--version`） |
| 30 Force | 删除未使用的 `--force`，或无 Force 且文件存在则 skip |

**发布文档（本轮建议做完）**

- 根目录 `LICENSE`（项目自身，选定 OSI 许可证）。
- `THIRD_PARTY_NOTICES.md`：yt-dlp、FFmpeg（含 Windows Gyan vs Unix eugeneware）来源、版本、许可证链接。

**本轮仍明确不做**

- Apple 公证、Windows Authenticode（与第一轮相同）。
- 把 nightly 指到 GitHub Latest。
- 系统凭据库（#4 后半）。
- 强行统一 Windows/Unix ffmpeg 主版本（先 notices + 差异说明；统一版本族另开需求，避免无回归测试时换二进制）。

---

## 建议排期

```text
PR-1 删除复活 + queued 按钮
PR-2 单 writer / 唯一 tmp / persist Result     } 第一阶段，1→2 串行，3/4 可并行
PR-3 ffmpeg override + 输出目录
PR-4 Release tag/nightly/质量门

PR-5 managed-only -U
PR-6 命令与 queue 脱敏                          } 第二阶段；5 依赖查找顺序（已有）

PR-7 TaskKind 参数 + resolve_effective_config
PR-8 playlist index + preview 上限
PR-9 Unix graceful kill                         } 第三阶段

P2 项与 LICENSE/notices 插入第四阶段
```

第一阶段做完即可再发 **新 tag**（例如 `v0.2.0`，且 tag 必须等于三份 version）。不要在 PR-4 前用错误 tag 补打。

---

## 最低验收矩阵

| 场景 | 必须 |
|---|---|
| 下载中暂停再立刻删除 | 列表不复活；queue.json 无此任务 |
| 排队中点删除/取消 | 任务消失或 canceled，不会再开始下载 |
| 20 线程写同一 settings/queue | 最终文件完整 JSON |
| 错误 ffmpeg 路径 | 立即失败，错误码可区分 |
| 输出目录只读 | 立即「无法创建输出目录」 |
| 更新引擎 | 只改 AppData；PATH 上的文件 hash 不变 |
| 命令预览 / queue.json | 看不到 proxy 密码 |
| 播放列表两条同 id | 可分别勾选 |
| 仅元数据工具 | 不带 --write-subs |
| tag `v0.2.0` 对代码 `0.1.0` | Release 失败 |
| nightly 构建失败 | 旧 nightly 仍可下载 |
| Unix pause | SIGTERM 后有等待，再 SIGKILL |

---

## 明确不做（除非另开需求）

- 本计划不包含 Apple 公证、Windows 代码签名
- 不把 nightly 做成 GitHub Latest
- 不在本轮接入系统钥匙串
- 不在没有跨平台 merge 回归测试时强行统一 ffmpeg 9.x / 6.x
- 不在 CI 用 xvfb 点完整 GUI
