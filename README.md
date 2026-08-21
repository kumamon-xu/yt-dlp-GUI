# yt-dlp GUI

基于 [yt-dlp](https://github.com/yt-dlp/yt-dlp) 的多平台视频解析下载器（B站 / YouTube / 抖音 / TikTok / 1000+ 站点）。

Tauri 2 + React + TypeScript + TailwindCSS。

## 开发

```bash
pnpm install
pnpm tauri dev      # 开发模式（热更新）
pnpm tauri build    # 打包
cargo test --manifest-path src-tauri/Cargo.toml
```

## 引擎

- `code/yt-dlp.exe`：yt-dlp 引擎（可 `yt-dlp -U` 自更新），缺失时应用会提示。
- `ffmpeg`：需在 PATH 中（合并视频 / 提取音频必需），<https://www.gyan.dev/ffmpeg/builds/>。

## 文档

- `VIBE_CODING_开发文档.md`：完整设计（调研 / 通信协议 / 架构 / 里程碑 M0-M8）。
- `CLAUDE.md`：AI 协作硬性规则。

## 里程碑状态

- [x] M0 脚手架 + 引擎/ffmpeg 检测
- [ ] M1 URL 预览（-J）
- [ ] M2 单任务下载（进度协议）
- [ ] M3 选项面板 + 命令预览
- [ ] M4 队列 + 持久化
- [ ] M5 播放列表/合集
- [ ] M6 工具盒（字幕/封面）
- [ ] M7 打磨
- [ ] M8 发布
