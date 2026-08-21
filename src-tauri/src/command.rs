//! 参数构造器：TaskConfig → Vec<String>（纯函数，可单测）

use serde::{Deserialize, Serialize};

pub use crate::parser::{DOWNLOAD_TPL, FILE_PRINT};

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Preset {
    #[default]
    Best,
    Mp4Prefer,
    Limit1080,
    Limit720,
    AudioMp3,
    AudioM4a,
    Custom(String),
}

impl Preset {
    pub fn format_expr(&self) -> Option<String> {
        Some(match self {
            Preset::Best => "bv*+ba/b".into(),
            Preset::Mp4Prefer => "bv*[ext=mp4]+ba[ext=m4a]/b[ext=mp4]/bv*+ba/b".into(),
            Preset::Limit1080 => "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b".into(),
            Preset::Limit720 => "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b".into(),
            Preset::AudioMp3 | Preset::AudioM4a => return None,
            Preset::Custom(e) => e.clone(),
        })
    }

    pub fn is_audio_extract(&self) -> bool {
        matches!(self, Preset::AudioMp3 | Preset::AudioM4a)
    }
}

/// 点选流 → customFormat（仅视频补 +ba，避免无声）
#[allow(dead_code)]
pub fn custom_format_for_stream(format_id: &str, has_video: bool, has_audio: bool) -> String {
    if has_video && !has_audio {
        format!("{format_id}+ba/{format_id}/b")
    } else {
        format_id.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct TaskConfig {
    pub url: String,
    pub preset: Preset,
    pub audio_quality: String,
    pub merge_format: String,
    pub out_dir: String,
    pub out_template: String,
    pub concurrent_fragments: u32,
    pub limit_rate: Option<String>,
    pub cookies_browser: Option<String>,
    pub cookies_file: Option<String>,
    pub proxy: Option<String>,
    pub embed_thumbnail: bool,
    pub embed_metadata: bool,
    pub write_subs: bool,
    pub sub_langs: Option<String>,
    pub embed_subs: bool,
    pub sponsorblock: bool,
    pub no_playlist: bool,
    pub playlist_items: Option<String>,
    pub resume: bool,
    pub ffmpeg_location: Option<String>,
    pub js_runtime: Option<String>,
    pub skip_download: bool,
    pub write_thumbnail: bool,
    pub convert_subs: Option<String>,
    pub write_info_json: bool,
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            preset: Preset::Best,
            audio_quality: "192K".into(),
            merge_format: String::new(),
            out_dir: String::new(),
            out_template: String::new(),
            concurrent_fragments: 4,
            limit_rate: None,
            cookies_browser: None,
            cookies_file: None,
            proxy: None,
            embed_thumbnail: false,
            embed_metadata: false,
            write_subs: false,
            sub_langs: None,
            embed_subs: false,
            sponsorblock: false,
            no_playlist: false,
            playlist_items: None,
            resume: false,
            ffmpeg_location: None,
            js_runtime: None,
            skip_download: false,
            write_thumbnail: false,
            convert_subs: None,
            write_info_json: false,
        }
    }
}

pub const DEFAULT_OUT_TEMPLATE: &str = "%(title)s [%(id)s].%(ext)s";

/// Cookies 文件优先于浏览器；空字符串视为未设置。下载与预览共用。
pub fn push_network_auth(
    a: &mut Vec<String>,
    cookies_file: Option<&str>,
    cookies_browser: Option<&str>,
    proxy: Option<&str>,
) {
    if let Some(f) = cookies_file.filter(|s| !s.is_empty()) {
        a.push("--cookies".into());
        a.push(f.to_string());
    } else if let Some(b) = cookies_browser.filter(|s| !s.is_empty()) {
        a.push("--cookies-from-browser".into());
        a.push(b.to_string());
    }
    if let Some(p) = proxy.filter(|s| !s.is_empty()) {
        a.push("--proxy".into());
        a.push(p.to_string());
    }
}

/// `yt-dlp -J` 参数（预览不下载）。认证规则与 `build_args` 相同。
pub fn build_preview_args(
    url: &str,
    cookies_file: Option<&str>,
    cookies_browser: Option<&str>,
    proxy: Option<&str>,
    js_runtime: Option<&str>,
) -> Vec<String> {
    let mut a = vec![
        "-J".into(),
        "--ignore-config".into(),
        "--no-color".into(),
        "--newline".into(),
        "--windows-filenames".into(),
    ];
    if let Some(js) = js_runtime.filter(|s| !s.is_empty()) {
        a.push("--js-runtimes".into());
        a.push(js.to_string());
    }
    push_network_auth(&mut a, cookies_file, cookies_browser, proxy);
    a.push(url.to_string());
    a
}

pub fn default_out_dir() -> String {
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE")
            .map(|p| format!("{p}\\Downloads"))
            .unwrap_or_else(|_| ".".into())
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME")
            .map(|p| format!("{p}/Downloads"))
            .unwrap_or_else(|_| ".".into())
    }
}

pub fn build_args(cfg: &TaskConfig) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();

    a.push("--no-color".into());
    a.push("--newline".into());
    a.push("--windows-filenames".into());
    a.push("--progress-delta".into());
    a.push("0.3".into());
    a.push("--progress-template".into());
    a.push(DOWNLOAD_TPL.into());
    a.push("-O".into());
    a.push(FILE_PRINT.into());
    a.push("--ignore-config".into());

    if let Some(ff) = cfg.ffmpeg_location.as_ref().filter(|s| !s.is_empty()) {
        a.push("--ffmpeg-location".into());
        a.push(ff.clone());
    }
    if let Some(js) = cfg.js_runtime.as_ref().filter(|s| !s.is_empty()) {
        a.push("--js-runtimes".into());
        a.push(js.clone());
    }

    match &cfg.preset {
        Preset::AudioMp3 => {
            a.push("-x".into());
            a.push("--audio-format".into());
            a.push("mp3".into());
            a.push("--audio-quality".into());
            a.push(if cfg.audio_quality.is_empty() {
                "192K".into()
            } else {
                cfg.audio_quality.clone()
            });
        }
        Preset::AudioM4a => {
            a.push("-x".into());
            a.push("--audio-format".into());
            a.push("m4a".into());
        }
        _ => {
            if let Some(f) = cfg.preset.format_expr() {
                a.push("-f".into());
                a.push(f);
            }
        }
    }

    if !cfg.preset.is_audio_extract() && !cfg.skip_download {
        let mf = if cfg.merge_format.is_empty() {
            "mp4"
        } else {
            cfg.merge_format.as_str()
        };
        a.push("--merge-output-format".into());
        a.push(mf.into());
    }

    let out_dir = if cfg.out_dir.is_empty() {
        default_out_dir()
    } else {
        cfg.out_dir.clone()
    };
    a.push("--paths".into());
    a.push(out_dir);
    a.push("-o".into());
    a.push(if cfg.out_template.is_empty() {
        DEFAULT_OUT_TEMPLATE.into()
    } else {
        cfg.out_template.clone()
    });

    let n = if cfg.concurrent_fragments == 0 {
        4
    } else {
        cfg.concurrent_fragments
    };
    a.push("-N".into());
    a.push(n.to_string());
    a.push("-R".into());
    a.push("10".into());
    if let Some(r) = &cfg.limit_rate {
        a.push("-r".into());
        a.push(r.clone());
    }

    push_network_auth(
        &mut a,
        cfg.cookies_file.as_deref(),
        cfg.cookies_browser.as_deref(),
        cfg.proxy.as_deref(),
    );

    if cfg.embed_thumbnail {
        a.push("--embed-thumbnail".into());
    }
    if cfg.embed_metadata {
        a.push("--embed-metadata".into());
    }
    if cfg.write_subs {
        a.push("--write-subs".into());
        a.push("--sub-langs".into());
        a.push(cfg.sub_langs.clone().unwrap_or_else(|| "zh.*,en.*".into()));
    }
    if let Some(c) = cfg.convert_subs.as_ref().filter(|s| !s.is_empty()) {
        a.push("--convert-subs".into());
        a.push(c.clone());
    }
    if cfg.embed_subs {
        a.push("--embed-subs".into());
    }
    if cfg.sponsorblock {
        a.push("--sponsorblock-remove".into());
        a.push("all".into());
    }
    if cfg.skip_download {
        a.push("--skip-download".into());
    }
    if cfg.write_thumbnail {
        a.push("--write-thumbnail".into());
    }
    if cfg.write_info_json {
        a.push("--write-info-json".into());
    }

    if cfg.no_playlist {
        a.push("--no-playlist".into());
    }
    if let Some(items) = &cfg.playlist_items {
        if !items.is_empty() {
            a.push("--playlist-items".into());
            a.push(items.clone());
        }
    }

    if cfg.resume {
        a.push("--continue".into());
    }

    a.push(cfg.url.clone());
    a
}

fn needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s.chars()
            .any(|c| c.is_whitespace() || "&()[]{}^=;!'+,`".contains(c))
}

pub fn quote_arg(s: &str) -> String {
    if needs_quotes(s) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

pub fn format_command(engine: &str, args: &[String]) -> String {
    std::iter::once(quote_arg(engine))
        .chain(args.iter().map(|a| quote_arg(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_preset_str() -> String {
    "mp4".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub url: String,
    #[serde(default = "default_preset_str")]
    pub preset: String,
    #[serde(default)]
    pub custom_format: Option<String>,
    #[serde(default)]
    pub audio_quality: Option<String>,
    #[serde(default)]
    pub merge_format: Option<String>,
    #[serde(default)]
    pub out_dir: Option<String>,
    #[serde(default)]
    pub out_template: Option<String>,
    #[serde(default)]
    pub concurrent_fragments: Option<u32>,
    #[serde(default)]
    pub limit_rate: Option<String>,
    #[serde(default)]
    pub cookies_browser: Option<String>,
    #[serde(default)]
    pub cookies_file: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub embed_thumbnail: Option<bool>,
    #[serde(default)]
    pub embed_metadata: Option<bool>,
    #[serde(default)]
    pub write_subs: Option<bool>,
    #[serde(default)]
    pub sub_langs: Option<String>,
    #[serde(default)]
    pub embed_subs: Option<bool>,
    #[serde(default)]
    pub sponsorblock: Option<bool>,
    #[serde(default)]
    pub no_playlist: Option<bool>,
    #[serde(default)]
    pub playlist_items: Option<String>,
    #[serde(default)]
    pub resume: Option<bool>,
    #[serde(default)]
    pub skip_download: Option<bool>,
    #[serde(default)]
    pub write_thumbnail: Option<bool>,
    #[serde(default)]
    pub convert_subs: Option<String>,
    #[serde(default)]
    pub write_info_json: Option<bool>,
}

impl NewTask {
    pub fn to_config(&self) -> TaskConfig {
        let preset = match self.preset.as_str() {
            "best" => Preset::Best,
            "1080p" => Preset::Limit1080,
            "720p" => Preset::Limit720,
            "mp3" => Preset::AudioMp3,
            "m4a" => Preset::AudioM4a,
            "custom" => Preset::Custom(
                self.custom_format
                    .clone()
                    .unwrap_or_else(|| "bv*+ba/b".into()),
            ),
            _ => Preset::Mp4Prefer, // 出厂默认 mp4（含空字符串）
        };
        TaskConfig {
            url: self.url.clone(),
            preset,
            audio_quality: self.audio_quality.clone().unwrap_or_else(|| "192K".into()),
            merge_format: self.merge_format.clone().unwrap_or_default(),
            out_dir: self.out_dir.clone().unwrap_or_default(),
            out_template: self.out_template.clone().unwrap_or_default(),
            concurrent_fragments: self.concurrent_fragments.unwrap_or(4),
            limit_rate: self.limit_rate.clone(),
            cookies_browser: self.cookies_browser.clone(),
            cookies_file: self.cookies_file.clone(),
            proxy: self.proxy.clone(),
            embed_thumbnail: self.embed_thumbnail.unwrap_or(false),
            embed_metadata: self.embed_metadata.unwrap_or(false),
            write_subs: self.write_subs.unwrap_or(false),
            sub_langs: self.sub_langs.clone(),
            embed_subs: self.embed_subs.unwrap_or(false),
            sponsorblock: self.sponsorblock.unwrap_or(false),
            no_playlist: self.no_playlist.unwrap_or(true),
            playlist_items: self.playlist_items.clone(),
            resume: self.resume.unwrap_or(false),
            ffmpeg_location: None,
            js_runtime: None,
            skip_download: self.skip_download.unwrap_or(false),
            write_thumbnail: self.write_thumbnail.unwrap_or(false),
            convert_subs: self.convert_subs.clone(),
            write_info_json: self.write_info_json.unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> TaskConfig {
        TaskConfig {
            url: "https://example.com/v".into(),
            preset: Preset::Best,
            ..Default::default()
        }
    }

    fn after<'a>(a: &'a [String], flag: &str) -> &'a str {
        let i = a.iter().position(|x| x == flag).expect(flag);
        &a[i + 1]
    }

    #[test]
    fn best_preset_format() {
        let a = build_args(&base());
        assert_eq!(after(&a, "-f"), "bv*+ba/b");
    }

    #[test]
    fn audio_mp3_uses_x_and_skips_merge() {
        let mut c = base();
        c.preset = Preset::AudioMp3;
        let a = build_args(&c);
        assert!(a.contains(&"-x".into()));
        assert_eq!(after(&a, "--audio-quality"), "192K");
        assert!(!a.contains(&"-f".into()));
        assert!(!a.iter().any(|x| x == "--merge-output-format"));
    }

    #[test]
    fn video_jobs_merge_mp4() {
        let a = build_args(&base());
        assert_eq!(after(&a, "--merge-output-format"), "mp4");
        let mut c = base();
        c.preset = Preset::Mp4Prefer;
        let a = build_args(&c);
        assert_eq!(after(&a, "--merge-output-format"), "mp4");
    }

    #[test]
    fn ffmpeg_location_present() {
        let mut c = base();
        c.ffmpeg_location = Some(r"D:\code\ffmpeg.exe".into());
        let a = build_args(&c);
        assert_eq!(after(&a, "--ffmpeg-location"), r"D:\code\ffmpeg.exe");
    }

    #[test]
    fn js_runtimes_present_when_configured() {
        let mut c = base();
        c.js_runtime = Some(r"node:C:\n\node.exe".into());
        let a = build_args(&c);
        assert_eq!(after(&a, "--js-runtimes"), r"node:C:\n\node.exe");
    }

    #[test]
    fn video_only_custom_format_contains_plus_ba() {
        let expr = custom_format_for_stream("137", true, false);
        assert_eq!(expr, "137+ba/137/b");
        let mut c = base();
        c.preset = Preset::Custom(expr);
        let a = build_args(&c);
        assert_eq!(after(&a, "-f"), "137+ba/137/b");
        assert_eq!(after(&a, "--merge-output-format"), "mp4");
    }

    #[test]
    fn cookies_file_wins_over_browser() {
        let mut c = base();
        c.cookies_file = Some("D:/cookies.txt".into());
        c.cookies_browser = Some("edge".into());
        let a = build_args(&c);
        assert!(a.contains(&"--cookies".into()));
        assert_eq!(after(&a, "--cookies"), "D:/cookies.txt");
        assert!(!a.iter().any(|x| x == "--cookies-from-browser"));
    }

    #[test]
    fn cookies_browser_when_no_file() {
        let mut c = base();
        c.cookies_browser = Some("edge".into());
        let a = build_args(&c);
        assert_eq!(after(&a, "--cookies-from-browser"), "edge");
    }

    #[test]
    fn resume_adds_continue() {
        let mut c = base();
        c.resume = true;
        let a = build_args(&c);
        assert!(a.contains(&"--continue".into()));
    }

    #[test]
    fn url_is_last() {
        let a = build_args(&base());
        assert_eq!(a.last().unwrap(), "https://example.com/v");
        assert_eq!(after(&a, "--progress-template"), DOWNLOAD_TPL);
    }

    #[test]
    fn empty_preset_defaults_to_mp4() {
        let t = NewTask {
            url: "https://x".into(),
            preset: String::new(),
            custom_format: None,
            audio_quality: None,
            merge_format: None,
            out_dir: None,
            out_template: None,
            concurrent_fragments: None,
            limit_rate: None,
            cookies_browser: None,
            cookies_file: None,
            proxy: None,
            embed_thumbnail: None,
            embed_metadata: None,
            write_subs: None,
            sub_langs: None,
            embed_subs: None,
            sponsorblock: None,
            no_playlist: None,
            playlist_items: None,
            resume: None,
            skip_download: None,
            write_thumbnail: None,
            convert_subs: None,
            write_info_json: None,
        };
        assert_eq!(t.to_config().preset, Preset::Mp4Prefer);
    }

    #[test]
    fn quote_command_wraps_spaces() {
        let s = format_command(r"C:\Program Files\yt-dlp.exe", &["-f".into(), "bv*+ba/b".into()]);
        assert!(s.starts_with('"'), "{s}");
        assert!(s.contains("yt-dlp.exe"));
    }

    #[test]
    fn preview_args_cookies_file_wins_and_proxy() {
        let a = build_preview_args(
            "https://www.bilibili.com/video/x",
            Some("D:/cookies.txt"),
            Some("edge"),
            Some("http://127.0.0.1:7890"),
            Some(r"node:C:\n\node.exe"),
        );
        assert_eq!(a[0], "-J");
        assert!(a.contains(&"--ignore-config".into()));
        assert_eq!(after(&a, "--cookies"), "D:/cookies.txt");
        assert!(!a.iter().any(|x| x == "--cookies-from-browser"));
        assert_eq!(after(&a, "--proxy"), "http://127.0.0.1:7890");
        assert_eq!(after(&a, "--js-runtimes"), r"node:C:\n\node.exe");
        assert_eq!(a.last().unwrap(), "https://www.bilibili.com/video/x");
    }

    #[test]
    fn preview_args_browser_cookies_when_no_file() {
        let a = build_preview_args("https://x", None, Some("chrome"), None, None);
        assert_eq!(after(&a, "--cookies-from-browser"), "chrome");
        assert!(!a.iter().any(|x| x == "--cookies"));
    }
}
