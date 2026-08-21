//! URL 元数据预览：yt-dlp -J（不下载）
//!
//! 返回轻量结构：单视频带精简 formats[]；播放列表带轻量 entries[]。
//! 重字段（format.url / http_headers / 全量描述）一律剥离，避免大 JSON 打爆 WebView。

use crate::errors::friendly_error;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// 单个格式（前端清晰度选择器数据源）
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

/// 播放列表条目（轻量）
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

/// 预览结果
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
        webpage_url: s(v, "webpage_url"),
        channel: s(v, "channel").or_else(|| s(v, "uploader")),
    })
}

/// 解析 yt-dlp -J 输出（纯函数，可单测）
pub fn parse_info_json(raw: &str) -> Result<VideoInfo, String> {
    let text = raw.trim();
    let start = text.find('{').ok_or_else(|| "返回内容为空".to_string())?;
    let v: Value = serde_json::from_str(&text[start..]).map_err(|e| format!("JSON 解析失败：{e}"))?;

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

    let playlist = entries.map(|arr| {
        arr.iter()
            .filter_map(|e| e.as_object().map(|_| e))
            .filter_map(extract_item)
            .collect()
    });

    // 描述截断，避免大文本
    let description = s(&v, "description").map(|d| {
        let chars: Vec<char> = d.chars().take(400).collect();
        let s: String = chars.into_iter().collect();
        if d.chars().count() > 400 { format!("{s}…") } else { s }
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
        playlist_title: if is_playlist { s(&v, "title").or_else(|| s(&v, "playlist")) } else { None },
        playlist_count: v
            .get("playlist_count")
            .and_then(|c| c.as_u64())
            .map(|c| c as usize),
    })
}

const PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);

/// PATH 中查找可执行文件（跨平台 where/which）
async fn js_runtime_available(name: &str) -> Option<std::path::PathBuf> {
    let lookup: &str = if cfg!(windows) { "where" } else { "which" };
    let out = crate::no_window_cmd(std::path::Path::new(lookup))
        .arg(name)
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
}

/// 解析 URL 元数据（不下载）
#[tauri::command]
pub async fn get_info(url: String, engine_path: Option<String>) -> Result<VideoInfo, String> {
    use crate::{find_engine, no_window_cmd};

    let engine = match engine_path.filter(|p| !p.is_empty()) {
        Some(p) => std::path::PathBuf::from(p),
        None => find_engine().ok_or_else(|| "未找到 yt-dlp 引擎".to_string())?,
    };

    // yt-dlp 2026+ 解析 YouTube 需要 JS 运行时（默认只启用 deno）；
    // 本机无 deno 但有 node 时显式启用 node。
    let mut cmd = no_window_cmd(&engine);
    cmd.arg("-J")
        .arg("--ignore-config")
        .arg("--no-color")
        .arg("--newline")
        .arg("--windows-filenames");
    if js_runtime_available("deno.exe").await.is_none() {
        if let Some(node) = js_runtime_available("node.exe").await {
            cmd.arg("--js-runtimes").arg(format!("node:{}", node.to_string_lossy()));
        }
    }
    cmd.arg(&url);
    let out = tokio::time::timeout(PREVIEW_TIMEOUT, cmd.output())
        .await
        .map_err(|_| "解析超时（30s）：链接可能无效，或需要网络/代理/Cookies".to_string())?
        .map_err(|e| friendly_error(&e.to_string()))?;

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
        assert_eq!(info.formats[0].resolution.as_deref(), Some("1920x1080"));
        assert!(info.formats[1].vcodec.is_none()); // "none" 已过滤
    }

    #[test]
    fn parse_playlist() {
        let raw = r#"{
            "_type": "playlist",
            "id": "PL1",
            "title": "我的合集",
            "playlist_count": 2,
            "entries": [
                {"id": "v1", "title": "第1集", "duration": 10.0, "thumbnail": "https://x/1.jpg"},
                {"id": "v2", "title": "第2集", "duration": 20.0, "thumbnail": "https://x/2.jpg"}
            ]
        }"#;
        let info = parse_info_json(raw).unwrap();
        assert!(info.is_playlist);
        assert_eq!(info.playlist.as_ref().unwrap().len(), 2);
        assert_eq!(info.playlist_count, Some(2));
    }
}
