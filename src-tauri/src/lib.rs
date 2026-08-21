//! yt-dlp GUI — Rust 核心
//! M0: 引擎( yt-dlp.exe )与 ffmpeg 检测；M1: URL 元数据预览
//!
//! 协议与约定见仓库根目录 VIBE_CODING_开发文档.md 与 CLAUDE.md。

mod errors;
mod info;

use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows 下 spawn 子进程不闪黑框（所有 spawn 必须走这里）
#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
pub(crate) fn no_window(_cmd: &mut Command) {}

// ---------------------------------------------------------------- 引擎定位

/// 按优先级查找 yt-dlp 可执行文件：
/// CWD/code → CWD/../code → exe 同级/code → exe 上级/code → PATH
pub(crate) fn find_engine() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            v.push(cwd.join("code").join(engine_bin_name()));
            v.push(cwd.join("..").join("code").join(engine_bin_name()));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                v.push(dir.join("code").join(engine_bin_name()));
                v.push(dir.join("..").join("code").join(engine_bin_name()));
            }
        }
        v
    };
    for c in candidates {
        let p = c.canonicalize().unwrap_or(c);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn engine_bin_name() -> &'static str {
    #[cfg(windows)]
    { "yt-dlp.exe" }
    #[cfg(not(windows))]
    { "yt-dlp" }
}

// ---------------------------------------------------------------- 状态结构

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub error: Option<String>,
}

/// 运行 `<bin> --version`（或自定义参数），取 stdout 首行作为版本号
async fn probe(bin: &Path, extra_args: &[&str]) -> ToolStatus {
    let bin_str = bin.to_string_lossy().to_string();
    let out = match no_window_cmd(bin).args(extra_args).output().await {
        Ok(o) => o,
        Err(e) => {
            return ToolStatus {
                available: false,
                path: Some(bin_str),
                version: None,
                error: Some(e.to_string()),
            }
        }
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("unknown error").to_string();
        return ToolStatus {
            available: false,
            path: Some(bin_str),
            version: None,
            error: Some(err),
        };
    }
    let first_line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    ToolStatus {
        available: true,
        path: Some(bin_str),
        version: Some(first_line),
        error: None,
    }
}

pub(crate) fn no_window_cmd(bin: &Path) -> Command {
    let mut cmd = Command::new(bin);
    no_window(&mut cmd);
    // UTF-8 输出（Windows GBK 控制台会搞坏中文标题）
    cmd.env("PYTHONUTF8", "1");
    cmd
}

// ---------------------------------------------------------------- 命令

/// 检测 yt-dlp 引擎
#[tauri::command]
async fn check_engine() -> Result<ToolStatus, String> {
    match find_engine() {
        Some(p) => Ok(probe(&p, &["--version"]).await),
        None => Err("未找到 yt-dlp 引擎：请在 code/ 目录放置 yt-dlp.exe，或在设置中指定路径".into()),
    }
}

/// 检测 ffmpeg（PATH 查找）
#[tauri::command]
async fn check_ffmpeg() -> Result<ToolStatus, String> {
    // Windows 上 where 查找
    let found = which_on_path("ffmpeg").await?;
    match found {
        Some(p) => Ok(probe(&p, &["-version"]).await),
        None => Err("未在 PATH 中找到 ffmpeg：合并视频/提取音频需要 ffmpeg（https://www.gyan.dev/ffmpeg/builds/）".into()),
    }
}

async fn which_on_path(name: &str) -> Result<Option<PathBuf>, String> {
    let lookup: &str = if cfg!(windows) { "where" } else { "which" };
    let out = no_window_cmd(Path::new(lookup))
        .arg(name)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().map(str::trim).filter(|s| !s.is_empty());
    Ok(first.map(|s| PathBuf::from(s)))
}

/// 检测 JS 运行时（yt-dlp 2026+ 解析 YouTube 需要 deno/node）
#[tauri::command]
async fn check_js_runtime() -> ToolStatus {
    let (deno_name, node_name) = if cfg!(windows) {
        ("deno.exe", "node.exe")
    } else {
        ("deno", "node")
    };
    // where/which 查找
    let find = |name: &str| -> Option<PathBuf> {
        let out = std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
            .arg(name)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout.lines().next().map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from)
    };
    if let Some(p) = find(deno_name) {
        return probe(&p, &["--version"]).await;
    }
    if let Some(p) = find(node_name) {
        return probe(&p, &["--version"]).await;
    }
    ToolStatus {
        available: false,
        path: None,
        version: None,
        error: Some("未找到 JS 运行时（deno/node）：YouTube 播放列表/频道页解析需要，推荐安装 deno 或 node".into()),
    }
}

/// 引擎可执行文件路径（供设置页展示）
#[tauri::command]
fn engine_path() -> Option<String> {
    find_engine().map(|p| p.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            check_engine,
            check_ffmpeg,
            check_js_runtime,
            engine_path,
            info::get_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
