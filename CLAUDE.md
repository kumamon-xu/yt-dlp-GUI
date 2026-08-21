# CLAUDE.md — 项目协作约定（给 AI 编程助手）

本项目 = yt-dlp 的图形化前端（Tauri 2 + React + TS + Tailwind + shadcn/ui）。
**先读 `VIBE_CODING_开发文档.md`，那是唯一事实来源（进度协议、架构、里程碑验收）。**

## 硬性规则
1. **禁止 shell 字符串拼接**。yt-dlp 参数一律数组传递（Rust `Vec<String>` / `tokio::process::Command` args）。
2. **进度协议常量只在 `src-tauri/src/parser.rs` 定义一次**：前缀 `YDLP|`、字段顺序
   `status|downloaded_bytes|total_bytes|speed|eta|title`。前端只认 `task_progress` / `task_log` 两个事件。
3. **参数构造必须是纯函数**：`build_args(task, settings) -> Vec<String>`，带单元测试；改 GUI 选项必须同步补测试。
4. 子进程固定头参数不可省略：`--no-color --newline --windows-filenames --progress-delta 0.3` + 两个 progress-template + `PYTHONUTF8=1` 环境变量 + Windows `CREATE_NO_WINDOW`。
5. 杀任务必须杀**进程树**（yt-dlp 会再 spawn ffmpeg），Windows 用 `taskkill /F /T`。
6. 任务持久化保存**完整 args 快照**（续传 = 原参数 + `--continue`）。
7. 不改动已完成里程碑的行为；每个里程碑单独 commit。
8. 错误输出必须走 `parser.rs` 的错误模式映射（文档 §7.6），禁止把原始英文错误直接抛给用户。

## 代码风格
- Rust：`thiserror` 错误类型，日志用 `tracing`；panic 不允许出现在任务路径上。
- TS：`strict` 模式；组件放 `src/components/`；状态集中 `src/store.ts`（Zustand）；预设定义在 `src/presets.ts`。
- 中文注释可以，英文标识符。UI 文案走 i18n key（zh 默认）。

## 本地验证命令（AI 改完自己跑）
```
pnpm tauri dev                      # 前端热更
pnpm tauri build                    # 打包
cargo test --manifest-path src-tauri/Cargo.toml
./code/yt-dlp.exe --version
./code/yt-dlp.exe -J --no-playlist "URL"   # 协议联调
```
