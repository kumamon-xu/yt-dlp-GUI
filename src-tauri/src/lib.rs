//! yt-dlp GUI — Rust 核心

mod command;
mod config;
mod info;
mod parser;
mod tasks;

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::Manager;
use tokio::process::Command;

pub use config::GlobalSettings;

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn set_resource_dir(dir: PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
pub(crate) fn no_window(_cmd: &mut Command) {}

#[cfg(unix)]
fn unix_process_group(cmd: &mut Command) {
    cmd.process_group(0);
}
#[cfg(not(unix))]
fn unix_process_group(_cmd: &mut Command) {}

#[cfg(windows)]
fn no_window_std(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
fn no_window_std(_cmd: &mut std::process::Command) {}

pub struct AppState {
    pub settings: Mutex<GlobalSettings>,
}

fn bin_name(base: &str) -> String {
    if cfg!(windows) && !base.ends_with(".exe") {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if is_tool_file(&p) {
            return Some(p);
        }
    }
    None
}

fn is_tool_file(p: &Path) -> bool {
    if !p.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = p.metadata() {
            if meta.permissions().mode() & 0o111 == 0 {
                let mut perms = meta.permissions();
                perms.set_mode(perms.mode() | 0o755);
                let _ = std::fs::set_permissions(p, perms);
            }
        }
    }
    true
}

fn push_dir_tools(candidates: &mut Vec<PathBuf>, dir: &Path, name: &str) {
    candidates.push(dir.join(name));
    candidates.push(dir.join("code").join(name));
}

/// resource_dir → CWD/code → exe 旁 → Contents/Resources (macOS) → PATH
pub(crate) fn find_tool(base: &str) -> Option<PathBuf> {
    let name = bin_name(base);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = RESOURCE_DIR.get() {
        push_dir_tools(&mut candidates, dir, &name);
    }
    if let Ok(cwd) = std::env::current_dir() {
        push_dir_tools(&mut candidates, &cwd, &name);
        candidates.push(cwd.join("..").join("code").join(&name));
        candidates.push(cwd.join(&name));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            push_dir_tools(&mut candidates, dir, &name);
            candidates.push(dir.join("..").join("code").join(&name));
            candidates.push(dir.join("resources").join("code").join(&name));
            candidates.push(dir.join("resources").join(&name));
            #[cfg(target_os = "macos")]
            {
                let resources = dir.join("..").join("Resources");
                push_dir_tools(&mut candidates, &resources, &name);
            }
        }
    }
    for c in candidates {
        let p = c.canonicalize().unwrap_or(c);
        if is_tool_file(&p) {
            return Some(p);
        }
    }
    which_on_path(&name)
}

pub(crate) fn find_engine(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    find_tool("yt-dlp")
}

pub(crate) fn find_ffmpeg(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    find_tool("ffmpeg")
}

pub(crate) fn js_runtime_arg() -> Option<String> {
    if which_on_path(&bin_name("deno")).is_some() {
        return None;
    }
    which_on_path(&bin_name("node")).map(|p| format!("node:{}", p.to_string_lossy()))
}

pub(crate) fn kill_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        no_window_std(&mut cmd);
        let _ = cmd.args(["/F", "/T", "/PID", &pid.to_string()]).status();
    }
    #[cfg(unix)]
    {
        // process_group(0) makes pid == pgid; negative kill targets the group (yt-dlp + ffmpeg).
        let pg = -(pid as i32);
        unsafe {
            libc::kill(pg, libc::SIGTERM);
            libc::kill(pg, libc::SIGKILL);
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub error: Option<String>,
}

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
        let err = String::from_utf8_lossy(&out.stderr)
            .lines()
            .next()
            .unwrap_or("unknown error")
            .to_string();
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
    unix_process_group(&mut cmd);
    cmd.env("PYTHONUTF8", "1");
    cmd
}

fn overrides(state: &AppState) -> (Option<String>, Option<String>) {
    state
        .settings
        .lock()
        .ok()
        .map(|s| (s.engine_path.clone(), s.ffmpeg_path.clone()))
        .unwrap_or((None, None))
}

#[tauri::command]
async fn check_engine(state: tauri::State<'_, AppState>) -> Result<ToolStatus, String> {
    let (eng, _) = overrides(&state);
    match find_engine(eng.as_deref()) {
        Some(p) => Ok(probe(&p, &["--version"]).await),
        None => Err("未找到 yt-dlp 引擎：请在 code/ 目录放置 yt-dlp，或在设置中指定路径".into()),
    }
}

#[tauri::command]
async fn check_ffmpeg(state: tauri::State<'_, AppState>) -> Result<ToolStatus, String> {
    let (_, ff) = overrides(&state);
    match find_ffmpeg(ff.as_deref()) {
        Some(p) => Ok(probe(&p, &["-version"]).await),
        None => Err("未找到 ffmpeg：请将 ffmpeg 放到 code/ 目录".into()),
    }
}

#[tauri::command]
async fn check_js_runtime() -> ToolStatus {
    if let Some(p) = which_on_path(&bin_name("deno")) {
        return probe(&p, &["--version"]).await;
    }
    if let Some(p) = which_on_path(&bin_name("node")) {
        return probe(&p, &["--version"]).await;
    }
    ToolStatus {
        available: false,
        path: None,
        version: None,
        error: Some("未找到 JS 运行时（deno/node）：YouTube 播放列表/频道页解析需要".into()),
    }
}

#[tauri::command]
fn engine_path(state: tauri::State<'_, AppState>) -> Option<String> {
    let (eng, _) = overrides(&state);
    find_engine(eng.as_deref()).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn ffmpeg_path(state: tauri::State<'_, AppState>) -> Option<String> {
    let (_, ff) = overrides(&state);
    find_ffmpeg(ff.as_deref()).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn build_command(state: tauri::State<'_, AppState>, task: command::NewTask) -> Result<String, String> {
    let (eng, ff) = overrides(&state);
    let engine = find_engine(eng.as_deref()).ok_or_else(|| "未找到 yt-dlp 引擎".to_string())?;
    let mut cfg = task.to_config();
    cfg.ffmpeg_location = find_ffmpeg(ff.as_deref()).map(|p| p.to_string_lossy().into());
    cfg.js_runtime = js_runtime_arg();
    let args = command::build_args(&cfg);
    Ok(command::format_command(&engine.to_string_lossy(), &args))
}

#[tauri::command]
async fn update_engine(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let (eng, _) = overrides(&state);
    let engine = find_engine(eng.as_deref()).ok_or_else(|| "未找到 yt-dlp 引擎".to_string())?;
    let out = no_window_cmd(&engine)
        .arg("-U")
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    Ok(format!("{stdout}{stderr}").trim().to_string())
}

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("no window icon")?;
    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("yt-dlp GUI")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                    let _ = w.unminimize();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                if let Some(w) = tray.app_handle().get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            settings: Mutex::new(GlobalSettings::default()),
        })
        .manage(tasks::TaskManager::default())
        .setup(|app| {
            if let Ok(dir) = app.path().resource_dir() {
                set_resource_dir(dir);
            }
            let s = config::load_from_disk(app.handle());
            if let Ok(mut g) = app.state::<AppState>().settings.lock() {
                *g = s;
            }
            tasks::restore_queue(app.handle());
            let _ = setup_tray(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_engine,
            check_ffmpeg,
            check_js_runtime,
            engine_path,
            ffmpeg_path,
            build_command,
            update_engine,
            info::get_info,
            tasks::start_task,
            tasks::cancel_task,
            tasks::remove_task,
            tasks::pause_task,
            tasks::resume_task,
            tasks::list_tasks,
            tasks::open_folder,
            config::load_settings,
            config::save_settings,
            config::pick_dir,
            config::pick_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_name_matches_platform() {
        #[cfg(windows)]
        {
            assert_eq!(bin_name("yt-dlp"), "yt-dlp.exe");
            assert_eq!(bin_name("ffmpeg"), "ffmpeg.exe");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(bin_name("yt-dlp"), "yt-dlp");
            assert_eq!(bin_name("ffmpeg"), "ffmpeg");
        }
    }

    #[test]
    fn kill_process_tree_ignores_pid_zero() {
        kill_process_tree(0);
    }
}
