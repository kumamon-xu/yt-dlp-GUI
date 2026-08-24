//! URL 元数据预览：yt-dlp -J（不下载）。新请求会杀掉上一次预览进程。

use crate::command::{build_preview_args, build_preview_args_flat};
use crate::parser::friendly_error;
use crate::AppState;
use serde::Serialize;
use serde_json::Value;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::process::Child;
use tokio::sync::Mutex;

/// 前端路径优先，否则用设置里的引擎覆盖（空串视为未设）。
pub fn resolve_engine_override(frontend: Option<&str>, settings: Option<&str>) -> Option<String> {
    frontend
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            settings
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

/// 预览结束时只清自己的 PID，避免冲掉更新的 -J 进程。
pub fn release_slot_if_owner(slot: &mut Option<u32>, pid: Option<u32>) {
    if pid.is_some() && *slot == pid {
        *slot = None;
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FormatInfo {
    pub format_id: String,
    pub ext: Option<String>,
    pub resolution: Option<String>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub filesize: Option<f64>,
    pub filesize_approx: Option<f64>,
    pub fps: Option<f64>,
    pub dynamic_range: Option<String>,
    pub protocol: Option<String>,
    pub container: Option<String>,
    pub note: Option<String>,
    pub language: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItem {
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub webpage_url: Option<String>,
    pub channel: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub uploader: Option<String>,
    pub description: Option<String>,
    pub webpage_url: Option<String>,
    pub is_playlist: bool,
    pub formats: Vec<FormatInfo>,
    pub playlist: Option<Vec<PlaylistItem>>,
    pub playlist_title: Option<String>,
    pub playlist_count: Option<usize>,
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty() && *s != "NA")
        .map(String::from)
}

fn f(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}

fn extract_format(v: &Value) -> Option<FormatInfo> {
    Some(FormatInfo {
        format_id: s(v, "format_id").unwrap_or_default(),
        ext: s(v, "ext"),
        resolution: s(v, "resolution"),
        vcodec: s(v, "vcodec").filter(|c| c != "none"),
        acodec: s(v, "acodec").filter(|c| c != "none"),
        filesize: f(v, "filesize"),
        filesize_approx: f(v, "filesize_approx"),
        fps: f(v, "fps"),
        dynamic_range: s(v, "dynamic_range").filter(|d| d != "SDR"),
        protocol: s(v, "protocol"),
        container: s(v, "container"),
        note: s(v, "note"),
        language: s(v, "language"),
    })
}

fn extract_item(v: &Value) -> Option<PlaylistItem> {
    Some(PlaylistItem {
        id: s(v, "id").unwrap_or_default(),
        title: s(v, "title").unwrap_or_else(|| s(v, "display_id").unwrap_or_default()),
        thumbnail: s(v, "thumbnail"),
        duration: f(v, "duration"),
        // --flat-playlist entries expose `url`, not `webpage_url`.
        webpage_url: s(v, "webpage_url").or_else(|| s(v, "url")),
        channel: s(v, "channel").or_else(|| s(v, "uploader")),
    })
}

pub fn parse_info_json(raw: &str) -> Result<VideoInfo, String> {
    let text = raw.trim();
    let start = text.find('{').ok_or_else(|| "返回内容为空".to_string())?;
    let v: Value =
        serde_json::from_str(&text[start..]).map_err(|e| format!("JSON 解析失败：{e}"))?;

    let entries = v
        .get("entries")
        .and_then(|e| e.as_array())
        .filter(|e| !e.is_empty());
    let is_playlist = v.get("_type").and_then(|t| t.as_str()) == Some("playlist")
        || (entries.is_some() && v.get("playlist_count").is_some());

    let formats = v
        .get("formats")
        .and_then(|f| f.as_array())
        .map(|arr| arr.iter().filter_map(extract_format).collect())
        .unwrap_or_default();

    let playlist = entries.map(|arr| arr.iter().filter_map(extract_item).collect());

    let description = s(&v, "description").map(|d| {
        let chars: Vec<char> = d.chars().take(400).collect();
        let s: String = chars.into_iter().collect();
        if d.chars().count() > 400 {
            format!("{s}…")
        } else {
            s
        }
    });

    Ok(VideoInfo {
        id: s(&v, "id").unwrap_or_default(),
        title: s(&v, "title").unwrap_or_else(|| s(&v, "url").unwrap_or_default()),
        thumbnail: s(&v, "thumbnail"),
        duration: f(&v, "duration"),
        uploader: s(&v, "uploader").or_else(|| s(&v, "channel")),
        description,
        webpage_url: s(&v, "webpage_url").or_else(|| s(&v, "url")),
        is_playlist,
        formats,
        playlist,
        playlist_title: if is_playlist {
            s(&v, "title").or_else(|| s(&v, "playlist"))
        } else {
            None
        },
        playlist_count: v
            .get("playlist_count")
            .and_then(|c| c.as_u64())
            .map(|c| c as usize),
    })
}

const PREVIEW_TIMEOUT_SINGLE: Duration = Duration::from_secs(30);
const PREVIEW_TIMEOUT_FLAT: Duration = Duration::from_secs(60);

fn preview_slot() -> &'static Mutex<Option<u32>> {
    static SLOT: OnceLock<Mutex<Option<u32>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[tauri::command]
pub async fn get_info(
    app: AppHandle,
    url: String,
    engine_path: Option<String>,
    flat: Option<bool>,
) -> Result<VideoInfo, String> {
    use crate::{find_engine, js_runtime_arg, kill_process_tree, no_window_cmd};

    let settings = app
        .state::<AppState>()
        .settings
        .lock()
        .ok()
        .map(|s| s.clone())
        .unwrap_or_default();

    if let Some(old) = preview_slot().lock().await.take() {
        kill_process_tree(old);
    }

    let engine_override =
        resolve_engine_override(engine_path.as_deref(), settings.engine_path.as_deref());
    let engine = find_engine(engine_override.as_deref())?;
    let use_flat = flat.unwrap_or(true);
    let args = if use_flat {
        build_preview_args_flat(
            &url,
            settings.cookies_file.as_deref(),
            settings.cookies_browser.as_deref(),
            settings.proxy.as_deref(),
            js_runtime_arg().as_deref(),
        )
    } else {
        build_preview_args(
            &url,
            settings.cookies_file.as_deref(),
            settings.cookies_browser.as_deref(),
            settings.proxy.as_deref(),
            js_runtime_arg().as_deref(),
        )
    };

    let mut cmd = no_window_cmd(&engine);
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child: Child = cmd.spawn().map_err(|e| friendly_error(&e.to_string()))?;
    let pid = child.id();
    if let Some(p) = pid {
        *preview_slot().lock().await = Some(p);
    }

    let timeout = if use_flat {
        PREVIEW_TIMEOUT_FLAT
    } else {
        PREVIEW_TIMEOUT_SINGLE
    };
    let wait = tokio::time::timeout(timeout, child.wait_with_output()).await;
    {
        let mut slot = preview_slot().lock().await;
        release_slot_if_owner(&mut slot, pid);
    }

    let out = match wait {
        Err(_) => {
            if let Some(p) = pid {
                kill_process_tree(p);
            }
            let secs = timeout.as_secs();
            return Err(format!(
                "解析超时（{secs}s）：链接可能无效，或需要网络/代理/Cookies"
            ));
        }
        Ok(Err(e)) => return Err(friendly_error(&e.to_string())),
        Ok(Ok(o)) => o,
    };

    if !out.status.success() {
        return Err(friendly_error(&String::from_utf8_lossy(&out.stderr)));
    }
    parse_info_json(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_video() {
        let raw = r#"{
            "id": "abc123",
            "title": "测试视频",
            "thumbnail": "https://x/t.jpg",
            "duration": 65.2,
            "uploader": "某人",
            "formats": [
                {"format_id": "bv", "ext": "mp4", "resolution": "1920x1080", "vcodec": "avc1", "acodec": "none", "filesize": 10485760, "fps": 30.0, "dynamic_range": "SDR"},
                {"format_id": "ba", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "filesize": 1048576}
            ]
        }"#;
        let info = parse_info_json(raw).unwrap();
        assert!(!info.is_playlist);
        assert_eq!(info.title, "测试视频");
        assert_eq!(info.formats.len(), 2);
        assert!(info.formats[1].vcodec.is_none());
    }

    #[test]
    fn parse_playlist() {
        let raw = r#"{
            "_type": "playlist",
            "id": "PL1",
            "title": "我的合集",
            "playlist_count": 2,
            "entries": [
                {"id": "v1", "title": "第1集", "duration": 10.0},
                {"id": "v2", "title": "第2集", "duration": 20.0}
            ]
        }"#;
        let info = parse_info_json(raw).unwrap();
        assert!(info.is_playlist);
        assert_eq!(info.playlist.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn flat_playlist_entries_use_url_when_no_webpage_url() {
        let raw = r#"{
            "_type": "playlist",
            "id": "PL",
            "title": "flat",
            "playlist_count": 1,
            "entries": [
                {"id": "v1", "title": "ep", "url": "https://example.com/watch?v=v1", "duration": 9.0}
            ]
        }"#;
        let info = parse_info_json(raw).unwrap();
        let item = &info.playlist.as_ref().unwrap()[0];
        assert_eq!(
            item.webpage_url.as_deref(),
            Some("https://example.com/watch?v=v1")
        );
    }

    #[test]
    fn parse_flat_playlist_thousand_entries() {
        let mut entries = String::new();
        for i in 0..1000 {
            if i > 0 {
                entries.push(',');
            }
            entries.push_str(&format!(r#"{{"id":"v{i}","title":"t{i}","duration":1.0}}"#));
        }
        let raw = format!(
            r#"{{"_type":"playlist","id":"PL","title":"big","playlist_count":1000,"entries":[{entries}]}}"#
        );
        let info = parse_info_json(&raw).unwrap();
        assert!(info.is_playlist);
        assert_eq!(info.playlist.as_ref().unwrap().len(), 1000);
        assert!(info.formats.is_empty());
    }

    #[test]
    fn engine_override_uses_settings_when_frontend_empty() {
        assert_eq!(
            resolve_engine_override(None, Some(r"D:\code\yt-dlp.exe")).as_deref(),
            Some(r"D:\code\yt-dlp.exe")
        );
        assert_eq!(
            resolve_engine_override(Some(""), Some(r"D:\code\yt-dlp.exe")).as_deref(),
            Some(r"D:\code\yt-dlp.exe")
        );
        assert_eq!(
            resolve_engine_override(Some(r"C:\custom\yt-dlp.exe"), Some(r"D:\code\yt-dlp.exe"))
                .as_deref(),
            Some(r"C:\custom\yt-dlp.exe")
        );
    }

    #[test]
    fn finishing_preview_does_not_wipe_newer_pid() {
        let mut slot = Some(2u32);
        release_slot_if_owner(&mut slot, Some(1));
        assert_eq!(slot, Some(2), "newer preview PID must stay");
        release_slot_if_owner(&mut slot, Some(2));
        assert_eq!(slot, None);
    }
}
