//! yt-dlp GUI — Rust 核心

#[cfg(all(test, windows))]
#[link(name = "resource", kind = "static")]
extern "C" {}

mod command;
mod config;
mod fsutil;
mod info;
mod locate;
mod parser;
mod redact;
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
    resolve_supported_js_runtime()
        .ok()
        .map(|r| format!("{}:{}", r.name, r.path.to_string_lossy()))
}

const DENO_MIN_VERSION: (u64, u64, u64) = (2, 3, 0);
const NODE_MIN_VERSION: (u64, u64, u64) = (22, 0, 0);

#[derive(Debug, Clone)]
struct SupportedJsRuntime {
    name: &'static str,
    path: PathBuf,
    source: locate::ToolSource,
    version: String,
}

fn parse_numeric_version(raw: &str) -> Option<(u64, u64, u64)> {
    let start = raw.find(|c: char| c.is_ascii_digit())?;
    let token: String = raw[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = token.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn validate_runtime_version(name: &str, version: &str) -> Result<(), String> {
    let minimum = match name {
        "deno" => DENO_MIN_VERSION,
        "node" => NODE_MIN_VERSION,
        _ => return Err(format!("未知 JS 运行时: {name}")),
    };
    match parse_numeric_version(version) {
        Some(actual) if actual >= minimum => Ok(()),
        Some(actual) => Err(format!(
            "{name} {}.{}.{} 版本过低，需要 >= {}.{}.{}",
            actual.0, actual.1, actual.2, minimum.0, minimum.1, minimum.2
        )),
        None => Err(format!("无法解析 {name} 版本: {version}")),
    }
}

fn probe_runtime_version(path: &Path) -> Result<String, String> {
    let mut cmd = std::process::Command::new(path);
    no_window_std(&mut cmd);
    let out = cmd
        .arg("--version")
        .output()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !out.status.success() {
        return Err(format!("{} --version failed", path.display()));
    }
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if line.is_empty() {
        return Err(format!("{} --version returned no version", path.display()));
    }
    Ok(line)
}

fn resolve_supported_js_runtime() -> Result<SupportedJsRuntime, String> {
    let path_only = locate::ToolLookup {
        allow_path: true,
        ..Default::default()
    };
    let mut issues = Vec::new();
    for name in ["deno", "node"] {
        let Ok((path, source)) = locate::locate_tool(name, None, &path_only) else {
            continue;
        };
        match probe_runtime_version(&path) {
            Ok(version) => match validate_runtime_version(name, &version) {
                Ok(()) => {
                    return Ok(SupportedJsRuntime {
                        name,
                        path,
                        source,
                        version,
                    });
                }
                Err(issue) => issues.push(issue),
            },
            Err(e) => issues.push(e),
        }
    }
    if issues.is_empty() {
        Err("未找到受支持的 JS 运行时（Deno >= 2.3 或 Node >= 22）".into())
    } else {
        Err(issues.join("；"))
    }
}

pub(crate) fn kill_process_tree(pid: u32) {
    kill_process_tree_grace(pid, std::time::Duration::ZERO);
}

pub(crate) fn kill_process_tree_grace(pid: u32, grace: std::time::Duration) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        let _ = grace;
        let mut cmd = std::process::Command::new("taskkill");
        no_window_std(&mut cmd);
        let _ = cmd.args(["/F", "/T", "/PID", &pid.to_string()]).status();
    }
    #[cfg(unix)]
    {
        let pg = -(pid as i32);
        unsafe {
            libc::kill(pg, libc::SIGTERM);
        }
        if !grace.is_zero() {
            let deadline = std::time::Instant::now() + grace;
            while std::time::Instant::now() < deadline {
                if !process_group_exists(pid) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
        unsafe {
            libc::kill(pg, libc::SIGKILL);
        }
    }
}

#[cfg(unix)]
fn process_group_exists(pid: u32) -> bool {
    let Ok(pgid) = i32::try_from(pid) else {
        return false;
    };
    let rc = unsafe { libc::kill(-pgid, 0) };
    if rc == 0 {
        return true;
    }
    matches!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    )
}

pub(crate) fn resolve_ffmpeg(
    override_path: Option<&str>,
    required: bool,
) -> Result<Option<PathBuf>, String> {
    let explicit = override_path.map(str::trim).filter(|s| !s.is_empty());
    if explicit.is_some() {
        return locate::locate_tool("ffmpeg", explicit, &current_lookup())
            .map(|(p, _)| Some(p))
            .map_err(|e| format!("FFMPEG_INVALID_OVERRIDE:{e}"));
    }
    match find_ffmpeg(None) {
        Ok(p) => Ok(Some(p)),
        Err(e) if required => Err(format!("FFMPEG_MISSING:{e}")),
        Err(_) => Ok(None),
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
    match resolve_supported_js_runtime() {
        Ok(r) => ToolStatus {
            available: true,
            path: Some(r.path.to_string_lossy().into_owned()),
            version: Some(r.version),
            error: None,
            source: Some(r.source),
        },
        Err(e) => ToolStatus {
            available: false,
            path: None,
            version: None,
            error: Some(e),
            source: None,
        },
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
    let settings = state
        .settings
        .lock()
        .ok()
        .map(|s| s.clone())
        .unwrap_or_default();
    let mut cfg = command::resolve_effective_config(task, &settings);
    let ff = resolve_ffmpeg(ff.as_deref(), command::needs_ffmpeg(&cfg))?;
    cfg.ffmpeg_location = ff.map(|p| p.to_string_lossy().into());
    cfg.js_runtime = js_runtime_arg();
    let args = command::build_args(&cfg);
    Ok(crate::redact::format_command_preview(
        &engine.to_string_lossy(),
        &args,
    ))
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
    if src == locate::ToolSource::Override {
        return Err(
            "ENGINE_UPDATE_EXTERNAL_OVERRIDE:当前使用自定义 yt-dlp 路径，请由外部工具管理更新"
                .into(),
        );
    }
    if src == locate::ToolSource::Managed {
        if is_under(&src_path, resource_dir) {
            return Err("拒绝修改安装包内的 bundled 引擎".into());
        }
        return Ok((src_path, locate::ToolSource::Managed));
    }
    let dir = managed_dir.ok_or_else(|| "未配置用户引擎目录".to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dest = dir.join(locate::bin_name("yt-dlp"));
    std::fs::copy(&src_path, &dest).map_err(|e| format!("无法复制到用户目录：{e}"))?;
    if is_under(&dest, resource_dir) {
        return Err("拒绝修改安装包内的 bundled 引擎".into());
    }
    Ok((dest, locate::ToolSource::Managed))
}

fn engine_update_lock() -> &'static tokio::sync::Mutex<()> {
    static L: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[tauri::command]
async fn update_engine(state: tauri::State<'_, AppState>) -> Result<EngineUpdateResult, String> {
    let _upd = engine_update_lock().lock().await;
    let (eng, _) = overrides(&state);
    let (src_path, src) = locate::locate_tool("yt-dlp", eng.as_deref(), &current_lookup())?;
    let old = probe(&src_path, &["--version"], src).await.version;
    let (target, new_src) =
        resolve_update_target(src_path.clone(), src, RESOURCE_DIR.get(), MANAGED_DIR.get())?;
    let backup = target.with_extension("bak");
    if target.exists() {
        let _ = std::fs::copy(&target, &backup);
    }
    let out = no_window_cmd(&target)
        .arg("-U")
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let message = format!("{stdout}{stderr}").trim().to_string();
    if !out.status.success() {
        if backup.exists() {
            let _ = std::fs::copy(&backup, &target);
        }
        return Err(if message.is_empty() {
            "yt-dlp -U 失败".into()
        } else {
            message
        });
    }
    let new = probe(&target, &["--version"], new_src).await.version;
    if new.is_none() {
        if backup.exists() {
            let _ = std::fs::copy(&backup, &target);
        }
        return Err("更新后 --version 失败，已回滚".into());
    }
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
            if let Ok(s) = config::load_from_disk(app.handle()) {
                if let Ok(mut g) = app.state::<AppState>().settings.lock() {
                    *g = s;
                }
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
            tasks::start_tasks,
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
    fn parses_supported_js_runtime_versions() {
        assert_eq!(parse_numeric_version("deno 2.3.0\nv8 13"), Some((2, 3, 0)));
        assert_eq!(parse_numeric_version("v22.11.0"), Some((22, 11, 0)));
        assert_eq!(parse_numeric_version("  v22.0.0\nextra"), Some((22, 0, 0)));
        assert_eq!(parse_numeric_version("node 24"), Some((24, 0, 0)));
        assert_eq!(parse_numeric_version("unknown"), None);
        assert!(parse_numeric_version("v22.0.0").unwrap() >= NODE_MIN_VERSION);
        assert!(parse_numeric_version("deno 2.2.9").unwrap() < DENO_MIN_VERSION);

        let candidates = [("deno", "deno 2.2.9"), ("node", "v22.0.0")];
        let selected = candidates
            .into_iter()
            .find(|(name, version)| validate_runtime_version(name, version).is_ok())
            .map(|(name, _)| name);
        assert_eq!(selected, Some("node"));
        assert!(validate_runtime_version("node", "v20.19.0").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn grace_period_tracks_the_whole_unix_process_group() {
        use std::os::unix::process::CommandExt;

        let dir = std::env::temp_dir().join(format!("ytdlp-process-group-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("child-survived");
        let child_pid_file = dir.join("child-pid");
        let mut command = std::process::Command::new("/bin/sh");
        command.process_group(0);
        let mut parent = command
            .env("SURVIVED_MARKER", &marker)
            .env("CHILD_PID_FILE", &child_pid_file)
            .args([
                "-c",
                "trap 'exit 0' TERM; (trap '' TERM; sleep 0.4; echo survived >\"$SURVIVED_MARKER\"; while :; do sleep 1; done) & echo $! >\"$CHILD_PID_FILE\"; wait",
            ])
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !child_pid_file.is_file() {
            assert!(std::time::Instant::now() < deadline, "child did not start");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        kill_process_tree_grace(parent.id(), std::time::Duration::from_millis(120));
        let _ = parent.wait();
        std::thread::sleep(std::time::Duration::from_millis(450));
        assert!(
            !marker.exists(),
            "descendant survived the grace-period kill"
        );
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn path_update_copies_to_managed_and_leaves_source() {
        let tmp = std::env::temp_dir().join(format!(
            "ytdlp-pathu-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let managed = tmp.join("engines");
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("on-path").join(locate::bin_name("yt-dlp"));
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"path-bytes").unwrap();
        let (dest, kind) = resolve_update_target(
            src.clone(),
            locate::ToolSource::Path,
            Some(&tmp.join("res")),
            Some(&managed),
        )
        .unwrap();
        assert_eq!(kind, locate::ToolSource::Managed);
        assert_eq!(std::fs::read(&src).unwrap(), b"path-bytes");
        assert_eq!(std::fs::read(&dest).unwrap(), b"path-bytes");
        assert_ne!(dest, src);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn override_update_is_rejected_without_copying() {
        let tmp = std::env::temp_dir().join(format!(
            "ytdlp-override-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let managed = tmp.join("engines");
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join(locate::bin_name("custom-ytdlp"));
        std::fs::write(&src, b"override-bytes").unwrap();
        let err = resolve_update_target(
            src.clone(),
            locate::ToolSource::Override,
            Some(&tmp.join("res")),
            Some(&managed),
        )
        .unwrap_err();
        assert!(err.contains("ENGINE_UPDATE_EXTERNAL_OVERRIDE"), "{err}");
        assert_eq!(std::fs::read(&src).unwrap(), b"override-bytes");
        assert!(!managed.exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn explicit_bad_ffmpeg_override_is_err() {
        let err = resolve_ffmpeg(Some(r"Z:\definitely-missing-ffmpeg\ffmpeg.exe"), true);
        assert!(err.is_err());
        let msg = err.unwrap_err();
        assert!(msg.contains("FFMPEG_INVALID_OVERRIDE"), "{msg}");
    }
}
