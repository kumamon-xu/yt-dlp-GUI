//! yt-dlp GUI — Rust 核心

mod command;
mod config;
mod fsutil;
mod info;
mod locate;
mod parser;
mod tasks;
mod validate;

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::Manager;
use tokio::process::Command;

pub use config::GlobalSettings;

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();
static MANAGED_DIR: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn set_resource_dir(dir: PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

pub(crate) fn set_managed_dir(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = MANAGED_DIR.set(dir);
}

pub(crate) fn current_lookup() -> locate::ToolLookup {
    locate::ToolLookup {
        resource_dir: RESOURCE_DIR.get().cloned(),
        managed_dir: MANAGED_DIR.get().cloned(),
        dev_code_dir: std::env::current_dir().ok().map(|c| c.join("code")),
        allow_path: true,
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

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
#[allow(dead_code)]
fn no_window_std(_cmd: &mut std::process::Command) {}

pub struct AppState {
    pub settings: Mutex<GlobalSettings>,
}

pub(crate) fn find_engine(override_path: Option<&str>) -> Result<PathBuf, String> {
    locate::locate_tool("yt-dlp", override_path, &current_lookup()).map(|(p, _)| p)
}

pub(crate) fn find_ffmpeg(override_path: Option<&str>) -> Result<PathBuf, String> {
    locate::locate_tool("ffmpeg", override_path, &current_lookup()).map(|(p, _)| p)
}

pub(crate) fn js_runtime_arg() -> Option<String> {
    let path_only = locate::ToolLookup {
        allow_path: true,
        ..Default::default()
    };
    if locate::locate_tool("deno", None, &path_only).is_ok() {
        return None;
    }
    locate::locate_tool("node", None, &path_only)
        .ok()
        .map(|(p, _)| format!("node:{}", p.to_string_lossy()))
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
    pub source: Option<locate::ToolSource>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EngineUpdateResult {
    pub updated: bool,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
    pub message: String,
    pub source: locate::ToolSource,
}

async fn probe(bin: &Path, extra_args: &[&str], source: locate::ToolSource) -> ToolStatus {
    let bin_str = bin.to_string_lossy().to_string();
    let out = match no_window_cmd(bin).args(extra_args).output().await {
        Ok(o) => o,
        Err(e) => {
            return ToolStatus {
                available: false,
                path: Some(bin_str),
                version: None,
                error: Some(e.to_string()),
                source: Some(source),
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
            source: Some(source),
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
        source: Some(source),
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
    let (p, src) = locate::locate_tool("yt-dlp", eng.as_deref(), &current_lookup())?;
    Ok(probe(&p, &["--version"], src).await)
}

#[tauri::command]
async fn check_ffmpeg(state: tauri::State<'_, AppState>) -> Result<ToolStatus, String> {
    let (_, ff) = overrides(&state);
    let (p, src) = locate::locate_tool("ffmpeg", ff.as_deref(), &current_lookup())?;
    Ok(probe(&p, &["-version"], src).await)
}

#[tauri::command]
async fn check_js_runtime() -> ToolStatus {
    let path_only = locate::ToolLookup {
        allow_path: true,
        ..Default::default()
    };
    if let Ok((p, src)) = locate::locate_tool("deno", None, &path_only) {
        return probe(&p, &["--version"], src).await;
    }
    if let Ok((p, src)) = locate::locate_tool("node", None, &path_only) {
        return probe(&p, &["--version"], src).await;
    }
    ToolStatus {
        available: false,
        path: None,
        version: None,
        error: Some("未找到 JS 运行时（deno/node）：YouTube 播放列表/频道页解析需要".into()),
        source: None,
    }
}

#[tauri::command]
fn engine_path(state: tauri::State<'_, AppState>) -> Option<String> {
    let (eng, _) = overrides(&state);
    find_engine(eng.as_deref())
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn ffmpeg_path(state: tauri::State<'_, AppState>) -> Option<String> {
    let (_, ff) = overrides(&state);
    find_ffmpeg(ff.as_deref())
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn build_command(
    state: tauri::State<'_, AppState>,
    task: command::NewTask,
) -> Result<String, String> {
    let (eng, ff) = overrides(&state);
    let engine = find_engine(eng.as_deref())?;
    let mut cfg = task.to_config();
    cfg.ffmpeg_location = find_ffmpeg(ff.as_deref())
        .ok()
        .map(|p| p.to_string_lossy().into());
    cfg.js_runtime = js_runtime_arg();
    let args = command::build_args(&cfg);
    Ok(command::format_command(&engine.to_string_lossy(), &args))
}

fn is_under(path: &Path, root: Option<&PathBuf>) -> bool {
    let Some(root) = root else {
        return false;
    };
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    path.starts_with(root)
}

pub(crate) fn resolve_update_target(
    src_path: PathBuf,
    src: locate::ToolSource,
    resource_dir: Option<&PathBuf>,
    managed_dir: Option<&PathBuf>,
) -> Result<(PathBuf, locate::ToolSource), String> {
    let target = if src == locate::ToolSource::Bundled {
        let dir = managed_dir.ok_or_else(|| "未配置用户引擎目录".to_string())?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let dest = dir.join(locate::bin_name("yt-dlp"));
        std::fs::copy(&src_path, &dest).map_err(|e| format!("无法复制到用户目录：{e}"))?;
        dest
    } else {
        src_path
    };
    if is_under(&target, resource_dir) {
        return Err("拒绝修改安装包内的 bundled 引擎".into());
    }
    let new_src = if src == locate::ToolSource::Bundled {
        locate::ToolSource::Managed
    } else {
        src
    };
    Ok((target, new_src))
}

#[tauri::command]
async fn update_engine(state: tauri::State<'_, AppState>) -> Result<EngineUpdateResult, String> {
    let (eng, _) = overrides(&state);
    let (src_path, src) = locate::locate_tool("yt-dlp", eng.as_deref(), &current_lookup())?;
    let old = probe(&src_path, &["--version"], src).await.version;
    let (target, new_src) =
        resolve_update_target(src_path, src, RESOURCE_DIR.get(), MANAGED_DIR.get())?;
    let out = no_window_cmd(&target)
        .arg("-U")
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let message = format!("{stdout}{stderr}").trim().to_string();
    if !out.status.success() {
        return Err(if message.is_empty() {
            "yt-dlp -U 失败".into()
        } else {
            message
        });
    }
    let new = probe(&target, &["--version"], new_src).await.version;
    Ok(EngineUpdateResult {
        updated: old != new,
        old_version: old,
        new_version: new,
        message,
        source: new_src,
    })
}

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = app.default_window_icon().cloned().ok_or("no window icon")?;
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
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            settings: Mutex::new(GlobalSettings::default()),
        })
        .manage(tasks::TaskManager::default())
        .setup(|app| {
            if let Ok(dir) = app.path().resource_dir() {
                set_resource_dir(dir);
            }
            if let Ok(dir) = app.path().app_local_data_dir() {
                set_managed_dir(dir.join("engines"));
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
    fn kill_process_tree_ignores_pid_zero() {
        kill_process_tree(0);
    }

    #[test]
    fn refuses_update_under_resource_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "ytdlp-res-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(tmp.join("code")).unwrap();
        let bundled = tmp.join("code").join(locate::bin_name("yt-dlp"));
        std::fs::write(&bundled, b"engine").unwrap();
        assert!(!is_under(&bundled, None));
        assert!(is_under(&bundled, Some(&tmp)));
        let err = resolve_update_target(
            bundled.clone(),
            locate::ToolSource::Override,
            Some(&tmp),
            Some(&tmp.join("engines")),
        );
        assert!(err.is_err(), "{err:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bundled_update_copies_to_managed_and_leaves_source() {
        let tmp = std::env::temp_dir().join(format!(
            "ytdlp-upd-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let resource = tmp.join("res");
        let managed = tmp.join("engines");
        std::fs::create_dir_all(resource.join("code")).unwrap();
        let src = resource.join("code").join(locate::bin_name("yt-dlp"));
        std::fs::write(&src, b"bundled-bytes").unwrap();
        let (dest, kind) = resolve_update_target(
            src.clone(),
            locate::ToolSource::Bundled,
            Some(&resource),
            Some(&managed),
        )
        .unwrap();
        assert_eq!(kind, locate::ToolSource::Managed);
        assert_eq!(dest, managed.join(locate::bin_name("yt-dlp")));
        assert_eq!(std::fs::read(&src).unwrap(), b"bundled-bytes");
        assert_eq!(std::fs::read(&dest).unwrap(), b"bundled-bytes");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
