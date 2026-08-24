//! 全局设置读写（JSON 落盘）+ 选目录

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettings {
    #[serde(default = "default_preset")]
    pub default_preset: String,
    #[serde(default)]
    pub out_dir: String,
    #[serde(default = "default_template")]
    pub out_template: String,
    #[serde(default = "default_n")]
    pub concurrent_fragments: u32,
    #[serde(default = "default_max")]
    pub max_concurrent_tasks: u32,
    #[serde(default)]
    pub limit_rate: Option<String>,
    #[serde(default)]
    pub cookies_browser: Option<String>,
    #[serde(default)]
    pub cookies_file: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub engine_path: Option<String>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default = "default_merge")]
    pub merge_format: String,
}

fn default_preset() -> String {
    "mp4".into()
}
fn default_template() -> String {
    crate::command::DEFAULT_OUT_TEMPLATE.into()
}
fn default_n() -> u32 {
    4
}
fn default_max() -> u32 {
    2
}
fn default_merge() -> String {
    "mp4".into()
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            default_preset: default_preset(),
            out_dir: crate::command::default_out_dir(),
            out_template: default_template(),
            concurrent_fragments: default_n(),
            max_concurrent_tasks: default_max(),
            limit_rate: None,
            cookies_browser: None,
            cookies_file: None,
            proxy: None,
            engine_path: None,
            ffmpeg_path: None,
            merge_format: default_merge(),
        }
    }
}

fn settings_save_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

pub fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法获取配置目录: {e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("无法创建配置目录 {}: {e}", dir.display()))?;
    Ok(dir)
}

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("settings.json"))
}

pub fn queue_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("queue.json"))
}

pub fn load_settings_file(p: &std::path::Path) -> Result<GlobalSettings, String> {
    match std::fs::read_to_string(p) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => Ok(v),
            Err(e) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let corrupt = p.with_file_name(format!("settings.corrupt-{ts}.json"));
                let _ = std::fs::rename(p, &corrupt);
                Err(format!("设置文件损坏，已改名为 {}: {e}", corrupt.display()))
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(GlobalSettings::default()),
        Err(e) => Err(format!("无法读取设置: {e}")),
    }
}

pub fn load_from_disk(app: &AppHandle) -> Result<GlobalSettings, String> {
    load_settings_file(&settings_path(app)?)
}

pub fn save_to_disk(app: &AppHandle, s: &GlobalSettings) -> Result<(), String> {
    crate::validate::validate_settings(s)?;
    crate::fsutil::atomic_write_json(&settings_path(app)?, s)
}

#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<GlobalSettings, String> {
    let s = load_from_disk(&app)?;
    if let Ok(mut g) = app.state::<crate::AppState>().settings.lock() {
        *g = s.clone();
    }
    Ok(s)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: GlobalSettings) -> Result<(), String> {
    let _lk = settings_save_lock()
        .lock()
        .map_err(|_| "settings save lock poisoned".to_string())?;
    crate::validate::validate_settings(&settings)?;
    save_to_disk(&app, &settings)?;
    let state = app.state::<crate::AppState>();
    let mut g = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;
    *g = settings;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_json_uses_camel_case() {
        let s = GlobalSettings::default();
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("defaultPreset").is_some());
        assert!(v.get("maxConcurrentTasks").is_some());
        assert!(v.get("concurrentFragments").is_some());
    }

    #[test]
    fn save_roundtrip_matches_in_memory() {
        let dir = std::env::temp_dir().join(format!(
            "ytdlp-settings-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let s = GlobalSettings {
            default_preset: "best".into(),
            max_concurrent_tasks: 3,
            proxy: Some("http://127.0.0.1:7890".into()),
            ..Default::default()
        };
        crate::validate::validate_settings(&s).unwrap();
        crate::fsutil::atomic_write_json(&path, &s).unwrap();
        let got: GlobalSettings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got.default_preset, s.default_preset);
        assert_eq!(got.max_concurrent_tasks, s.max_concurrent_tasks);
        assert_eq!(got.proxy, s.proxy);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_settings_are_renamed() {
        let dir = std::env::temp_dir().join(format!(
            "ytdlp-corrupt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        std::fs::write(&p, b"{not json").unwrap();
        let err = load_settings_file(&p);
        assert!(err.is_err(), "{err:?}");
        assert!(!p.exists());
        let renamed: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("settings.corrupt-"))
            .collect();
        assert_eq!(renamed.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

fn file_path_string(p: tauri_plugin_dialog::FilePath) -> String {
    p.into_path()
        .map(|pb| pb.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pick_dir(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app.dialog().file().blocking_pick_folder();
    Ok(picked.map(file_path_string).filter(|s| !s.is_empty()))
}

#[tauri::command]
pub async fn pick_file(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app.dialog().file().blocking_pick_file();
    Ok(picked.map(file_path_string).filter(|s| !s.is_empty()))
}
