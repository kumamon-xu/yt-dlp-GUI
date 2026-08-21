//! 全局设置读写（JSON 落盘）+ 选目录

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

pub fn settings_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("settings.json")
}

pub fn queue_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("queue.json")
}

pub fn load_from_disk(app: &AppHandle) -> GlobalSettings {
    let p = settings_path(app);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_to_disk(app: &AppHandle, s: &GlobalSettings) -> Result<(), String> {
    let p = settings_path(app);
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(p, json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_settings(app: AppHandle) -> GlobalSettings {
    let s = load_from_disk(&app);
    if let Ok(mut g) = app.state::<crate::AppState>().settings.try_lock() {
        *g = s.clone();
    }
    s
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: GlobalSettings) -> Result<(), String> {
    save_to_disk(&app, &settings)?;
    if let Ok(mut g) = app.state::<crate::AppState>().settings.try_lock() {
        *g = settings;
    }
    Ok(())
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
