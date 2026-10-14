//! HLS routing and N_m3u8DL-RE arguments. Resolved URLs stay in memory only.

use crate::command::{Preset, TaskConfig};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct HlsSource {
    pub url: String,
    pub title: String,
    pub filename: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub resolved: bool,
    pub cookie_text: Option<String>,
}

pub fn is_m3u8_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https") && u.path().to_ascii_lowercase().ends_with(".m3u8")
    })
}

pub fn eligible(cfg: &TaskConfig) -> bool {
    !cfg.skip_download
        && !cfg.preset.is_audio_extract()
        && !cfg.embed_thumbnail
        && !cfg.embed_metadata
        && !cfg.write_subs
        && !cfg.embed_subs
        && !cfg.sponsorblock
        && !cfg.write_thumbnail
        && !cfg.write_info_json
}

pub fn direct_auto(cfg: &TaskConfig) -> bool {
    is_m3u8_url(&cfg.url) && !matches!(cfg.preset, Preset::Custom(_))
}

pub fn direct_source(url: &str) -> HlsSource {
    let title = url::Url::parse(url)
        .ok()
        .and_then(|u| u.path_segments()?.next_back().map(|s| s.to_string()))
        .unwrap_or_else(|| "HLS video".into());
    HlsSource {
        url: url.into(),
        title: title.trim_end_matches(".m3u8").into(),
        filename: None,
        headers: BTreeMap::new(),
        resolved: false,
        cookie_text: None,
    }
}

pub fn resolver_args(cfg: &TaskConfig, browser_cookie_export: Option<&Path>) -> Vec<String> {
    let mut args = crate::command::build_preview_args(
        &cfg.url,
        cfg.cookies_file.as_deref(),
        cfg.cookies_browser.as_deref(),
        cfg.proxy.as_deref(),
        cfg.js_runtime.as_deref(),
    );
    args.pop();
    if let Some(path) = browser_cookie_export {
        args.extend(["--cookies".into(), path.to_string_lossy().into_owned()]);
    }
    if cfg.no_playlist {
        args.push("--no-playlist".into());
    }
    // Do not expand an unbounded channel/playlist just to decide which engine to use.
    else {
        args.push("--flat-playlist".into());
    }
    if let Some(items) = &cfg.playlist_items {
        args.extend(["--playlist-items".into(), items.clone()]);
    }
    let original = cfg.preset.format_expr().unwrap_or_else(|| "b".into());
    let hls = match cfg.preset {
        Preset::Limit720 => {
            "bv*[protocol^=m3u8][height<=720]+ba[protocol^=m3u8]/b[protocol^=m3u8][height<=720]"
        }
        Preset::Limit1080 => {
            "bv*[protocol^=m3u8][height<=1080]+ba[protocol^=m3u8]/b[protocol^=m3u8][height<=1080]"
        }
        Preset::Custom(_) => "",
        _ => "bv*[protocol^=m3u8]+ba[protocol^=m3u8]/b[protocol^=m3u8]",
    };
    args.extend([
        "-f".into(),
        if hls.is_empty() {
            original
        } else {
            format!("{hls}/{original}")
        },
        "-P".into(),
        cfg.out_dir.clone(),
        "-o".into(),
        if cfg.out_template.is_empty() {
            crate::command::DEFAULT_OUT_TEMPLATE.into()
        } else {
            cfg.out_template.clone()
        },
        "--print".into(),
        "%(filename)j".into(),
        cfg.url.clone(),
    ]);
    args
}

pub fn parse_resolver_output(raw: &[u8]) -> Result<Value, String> {
    // --print filename gives the fully expanded yt-dlp output template; -J follows it.
    let mut filename = None;
    let mut info = None;
    for value in serde_json::Deserializer::from_slice(raw).into_iter::<Value>() {
        let value = value.map_err(|e| format!("HLS 解析结果无效：{e}"))?;
        match value {
            Value::String(s) => filename = Some(s),
            Value::Object(_) => info = Some(value),
            _ => {}
        }
    }
    let mut info = info.ok_or_else(|| "视频解析结果为空".to_string())?;
    if let Some(filename) = filename {
        info["_filename"] = Value::String(filename);
    }
    Ok(info)
}

fn hls_format(v: &Value) -> bool {
    v["protocol"]
        .as_str()
        .is_some_and(|s| s.starts_with("m3u8"))
        || v["url"].as_str().is_some_and(is_m3u8_url)
}

pub fn source_from_info(v: &Value) -> Option<HlsSource> {
    if v["_type"] == "playlist" || v.get("entries").is_some() {
        return None;
    }
    let formats: Vec<&Value> = v["requested_formats"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_else(|| vec![v]);
    // RE can safely consume one selected media playlist verbatim. Several tracks
    // need independent URLs/headers and an exact mapping; keep those on yt-dlp.
    if formats.len() != 1
        || formats
            .iter()
            .any(|f| !hls_format(f) || f["has_drm"] == true)
    {
        return None;
    }
    let video = formats[0];
    let stream = video["url"].as_str()?;
    // A URL pointing back to the master still lets RE choose different tracks.
    if video["manifest_url"].as_str() == Some(stream) {
        return None;
    }
    let parsed = url::Url::parse(stream).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let mut headers = BTreeMap::new();
    for obj in [v.get("http_headers"), video.get("http_headers")]
        .into_iter()
        .flatten()
    {
        if let Some(obj) = obj.as_object() {
            for (k, val) in obj {
                if let Some(val) = val.as_str() {
                    headers.insert(k.clone(), val.into());
                }
            }
        }
    }
    // yt-dlp may serialize its scoped cookies separately from http_headers.
    if let Some(cookies) = video["cookies"].as_str().or_else(|| v["cookies"].as_str()) {
        let pairs: Vec<_> = cookies
            .split(';')
            .map(str::trim)
            .filter(|s| {
                s.contains('=')
                    && !["domain=", "path=", "expires=", "version="]
                        .iter()
                        .any(|prefix| s.to_ascii_lowercase().starts_with(prefix))
            })
            .collect();
        if !pairs.is_empty() {
            headers.insert("Cookie".into(), pairs.join("; "));
        }
    }
    if !headers.keys().any(|k| k.eq_ignore_ascii_case("referer")) {
        if let Some(page) = v["webpage_url"].as_str() {
            headers.insert("Referer".into(), page.into());
        }
    }
    Some(HlsSource {
        url: stream.into(),
        title: v["title"].as_str().unwrap_or("HLS video").into(),
        filename: v["_filename"]
            .as_str()
            .or_else(|| v["filename"].as_str())
            .map(String::from),
        headers,
        resolved: true,
        cookie_text: None,
    })
}

pub fn cookie_header(text: &str, target: &str, now: u64) -> String {
    let Ok(u) = url::Url::parse(target) else {
        return String::new();
    };
    let host = u.host_str().unwrap_or("");
    text.lines()
        .filter_map(|line| {
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') {
                return None;
            }
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 7 {
                return None;
            }
            let domain = fields[0].trim_start_matches('.');
            let domain_ok =
                host == domain || (fields[1] == "TRUE" && host.ends_with(&format!(".{domain}")));
            let cookie_path = fields[2];
            let path_ok = u.path() == cookie_path
                || (u.path().starts_with(cookie_path)
                    && (cookie_path.ends_with('/')
                        || u.path().as_bytes().get(cookie_path.len()) == Some(&b'/')));
            let expiry = fields[4].parse::<u64>().ok()?;
            if !domain_ok
                || !path_ok
                || (fields[3] == "TRUE" && u.scheme() != "https")
                || (expiry != 0 && expiry <= now)
            {
                return None;
            }
            Some(format!("{}={}", fields[5], fields[6]))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn safe_name(raw: &str) -> String {
    let name: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .scan(0usize, |bytes, c| {
            *bytes += c.len_utf8();
            (*bytes <= 160).then_some(c)
        })
        .collect();
    let name = name.trim_matches([' ', '.']);
    if name.is_empty() {
        "HLS video".into()
    } else {
        name.into()
    }
}

pub fn output_base(cfg: &TaskConfig, source: &HlsSource, _id: &str) -> PathBuf {
    if let Some(filename) = &source.filename {
        let mut path = PathBuf::from(filename);
        path.set_extension("");
        if !path.is_absolute() && !path.starts_with(&cfg.out_dir) {
            path = Path::new(&cfg.out_dir).join(path);
        }
        return path;
    }
    // Direct manifests have no webpage metadata to expand arbitrary yt-dlp template fields.
    Path::new(&cfg.out_dir).join(safe_name(&source.title))
}

pub fn build_args(
    cfg: &TaskConfig,
    source: &HlsSource,
    base: &Path,
    tmp: &Path,
) -> Result<Vec<String>, String> {
    let format = match cfg.merge_format.as_str() {
        "" | "mp4" => "mp4",
        "mkv" => "mkv",
        "ts" => "ts",
        other => {
            return Err(format!(
                "N_m3u8DL-RE 不支持输出容器 {other}，请选择 mp4、mkv 或 ts"
            ))
        }
    };
    let mut args: Vec<String> = vec![
        source.url.clone(),
        "--auto-select".into(),
        "--no-ansi-color".into(),
        "--force-ansi-console".into(),
        "--no-log".into(),
        "--write-meta-json".into(),
        "false".into(),
        "--disable-update-check".into(),
        "--ui-language".into(),
        "en-US".into(),
        "--thread-count".into(),
        cfg.concurrent_fragments.to_string(),
        "--save-dir".into(),
        base.parent()
            .unwrap_or(Path::new("."))
            .to_string_lossy()
            .into(),
        "--save-name".into(),
        base.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        "--tmp-dir".into(),
        tmp.to_string_lossy().into(),
        "--mux-after-done".into(),
        format!("format={format}:muxer=ffmpeg"),
    ];
    if let Some(ffmpeg) = &cfg.ffmpeg_location {
        args.extend(["--ffmpeg-binary-path".into(), ffmpeg.clone()]);
    }
    if !source.resolved && matches!(cfg.preset, Preset::Limit720 | Preset::Limit1080) {
        // Drop only known resolutions above the cap. Single media playlists often
        // have no resolution metadata; filtering them out would silently lose video.
        let heights = if cfg.preset == Preset::Limit720 {
            r"72[1-9]|7[3-9][0-9]|[89][0-9]{2}|[1-9][0-9]{3,}"
        } else {
            r"108[1-9]|109[0-9]|1[1-9][0-9]{2}|[2-9][0-9]{3}|[1-9][0-9]{4,}"
        };
        args.extend([
            "--drop-video".into(),
            format!("res=.*x({heights})$:for=all"),
        ]);
    }
    if let Some(proxy) = &cfg.proxy {
        args.extend(["--custom-proxy".into(), proxy.clone()]);
    } else {
        args.extend(["--use-system-proxy".into(), "false".into()]);
    }
    if let Some(rate) = &cfg.limit_rate {
        let (num, factor) = match rate.as_bytes().last() {
            Some(b'K') => (&rate[..rate.len() - 1], 1024.),
            Some(b'M') => (&rate[..rate.len() - 1], 1024. * 1024.),
            Some(b'G') => (&rate[..rate.len() - 1], 1024. * 1024. * 1024.),
            _ => (rate.as_str(), 1.),
        };
        let bytes = num
            .parse::<f64>()
            .map_err(|_| "无效的下载限速".to_string())?
            * factor;
        if bytes <= 0. || !bytes.is_finite() {
            return Err("下载限速必须大于 0".into());
        }
        // v0.6.0-beta parses K/M as binary byte rates, despite its CLI help.
        args.extend(["--max-speed".into(), format!("{}K", bytes / 1024.)]);
    }
    for (name, value) in &source.headers {
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err("无效的 HTTP 请求头".into());
        }
        args.extend(["--header".into(), format!("{name}: {value}")]);
    }
    Ok(args)
}

pub fn clean_console(raw: &str) -> String {
    static ANSI: OnceLock<regex::Regex> = OnceLock::new();
    ANSI.get_or_init(|| {
        regex::Regex::new(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\))").unwrap()
    })
    .replace_all(raw, "")
    .into_owned()
}

pub fn progress(line: &str) -> Option<f64> {
    static PERCENT: OnceLock<regex::Regex> = OnceLock::new();
    let re = PERCENT.get_or_init(|| regex::Regex::new(r"(\d+(?:\.\d+)?)%").unwrap());
    re.captures(line)?
        .get(1)?
        .as_str()
        .parse::<f64>()
        .ok()
        .map(|p| p.clamp(0., 100.))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn detects_manifest_path_only() {
        assert!(is_m3u8_url("https://cdn.test/INDEX.M3U8?token=x#part"));
        assert!(!is_m3u8_url("https://site.test/watch?next=x.m3u8"));
        assert!(!is_m3u8_url("file:///x.m3u8"));
    }
    #[test]
    fn multiple_tracks_use_ytdlp_instead_of_reselecting_the_master() {
        let v = json!({"title":"Video", "webpage_url":"https://site.test/watch", "http_headers":{"User-Agent":"test"},
            "requested_formats":[{"protocol":"m3u8_native","vcodec":"h264","width":1280,"height":720,"url":"https://cdn.test/v.m3u8","manifest_url":"https://cdn.test/master.m3u8"},
                {"protocol":"m3u8_native","vcodec":"none","url":"https://cdn.test/a.m3u8","manifest_url":"https://cdn.test/master.m3u8"}]});
        assert!(source_from_info(&v).is_none());
        let mut mixed = v.clone();
        mixed["requested_formats"][1]["protocol"] = json!("https");
        mixed["requested_formats"][1]["url"] = json!("https://cdn.test/a.m4a");
        assert!(source_from_info(&mixed).is_none());
        let mut separate = v;
        separate["requested_formats"][1]["manifest_url"] = json!("https://cdn.test/other.m3u8");
        assert!(source_from_info(&separate).is_none());
    }
    #[test]
    fn cookies_respect_scope_and_expiry() {
        let text = ".test.com\tTRUE\t/video\tTRUE\t0\ta\tsecret\n#HttpOnly_cdn.test.com\tFALSE\t/\tTRUE\t200\tb\tx\ncdn.test.com\tFALSE\t/\tFALSE\t1\told\ty";
        assert_eq!(
            cookie_header(text, "https://cdn.test.com/video/index.m3u8", 100),
            "a=secret; b=x"
        );
        assert_eq!(cookie_header(text, "http://cdn.test.com/videos", 100), "");
        assert_eq!(cookie_header(text, "https://other.com/video", 100), "");
    }
    #[test]
    fn translates_rate_and_does_not_pass_ytdlp_flags() {
        let cfg = TaskConfig {
            limit_rate: Some("1M".into()),
            ffmpeg_location: Some("C:/ffmpeg.exe".into()),
            ..Default::default()
        };
        let args = build_args(
            &cfg,
            &direct_source("https://test/index.m3u8"),
            Path::new("out/video"),
            Path::new("tmp"),
        )
        .unwrap();
        let i = args.iter().position(|s| s == "--max-speed").unwrap();
        assert_eq!(args[i + 1], "1024K");
        assert!(!args
            .iter()
            .any(|s| s == "--cookies-from-browser" || s == "--continue"));
        assert!(args.contains(&"--auto-select".into()));
    }

    #[test]
    fn rate_units_match_the_pinned_re_byte_rate_parser() {
        for (rate, expected_kib) in [
            ("1M", 1024.),
            ("500K", 500.),
            ("1.5M", 1536.),
            ("2G", 2097152.),
            ("1024", 1.),
        ] {
            let cfg = TaskConfig {
                limit_rate: Some(rate.into()),
                ..Default::default()
            };
            let args = build_args(
                &cfg,
                &direct_source("https://test/index.m3u8"),
                Path::new("out/video"),
                Path::new("tmp"),
            )
            .unwrap();
            let index = args.iter().position(|s| s == "--max-speed").unwrap();
            let kib = args[index + 1]
                .strip_suffix('K')
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert_eq!(kib, expected_kib, "{rate}");
        }
    }

    #[test]
    fn one_selected_playlist_preserves_audio_codec_fps_and_headers() {
        for (url, vcodec, acodec) in [
            ("audio-en", "none", "mp4a.40.2"),
            ("avc-30", "avc1", "none"),
            ("hevc-60", "hev1", "mp4a.40.2"),
        ] {
            let exact = format!("https://cdn.test/{url}.m3u8?token=chosen");
            let v = json!({ "title":"Selected", "protocol":"m3u8_native", "url":exact,
                "manifest_url":"https://cdn.test/master.m3u8", "vcodec":vcodec, "acodec":acodec,
                "width":1280, "height":720, "fps":30,
                "webpage_url":"https://site.test/watch", "http_headers":{"User-Agent":"test"} });
            let source = source_from_info(&v).unwrap();
            assert_eq!(source.url, exact);
            assert!(source.resolved);
            assert_eq!(source.headers["Referer"], "https://site.test/watch");
            let cfg = TaskConfig {
                preset: Preset::Limit720,
                ..Default::default()
            };
            let args =
                build_args(&cfg, &source, Path::new("out/selected"), Path::new("tmp")).unwrap();
            assert_eq!(args[0], exact);
            assert!(!args
                .iter()
                .any(|s| s == "--select-video" || s == "--drop-video"));
        }
    }

    #[test]
    fn an_explicit_selection_must_not_fall_back_to_master_auto_selection() {
        let cfg = TaskConfig {
            url: "https://test/master.m3u8".into(),
            preset: Preset::Custom("audio-en".into()),
            ..Default::default()
        };
        assert!(!direct_auto(&cfg));
        assert!(direct_auto(&TaskConfig {
            preset: Preset::Best,
            ..cfg
        }));
        assert!(source_from_info(
            &json!({"protocol":"m3u8_native","url":"https://test/master.m3u8",
            "manifest_url":"https://test/master.m3u8"})
        )
        .is_none());
    }
    #[test]
    fn strips_ansi_and_reads_percent() {
        assert_eq!(
            clean_console("\x1b[32mVid 12/20 60.00%\x1b[0m"),
            "Vid 12/20 60.00%"
        );
        assert_eq!(progress("Vid 12/20 60.00%"), Some(60.));
    }

    #[test]
    fn resolver_output_keeps_expanded_filename() {
        let v = parse_resolver_output(
            br#""out/Video.with.dots.mp4"
            {"protocol":"m3u8_native","url":"https://test/stream"}"#,
        )
        .unwrap();
        let source = source_from_info(&v).unwrap();
        let cfg = TaskConfig {
            out_dir: "out".into(),
            ..Default::default()
        };
        assert_eq!(
            output_base(&cfg, &source, "task"),
            Path::new("out/Video.with.dots")
        );
        assert!(parse_resolver_output(b"garbage").is_err());
    }

    #[test]
    fn custom_formats_and_ytdlp_only_features_are_preserved() {
        let cfg = TaskConfig {
            url: "https://test/watch".into(),
            preset: Preset::Custom("id-720+ba".into()),
            ..Default::default()
        };
        let args = resolver_args(&cfg, None);
        let index = args.iter().position(|s| s == "-f").unwrap();
        assert_eq!(args[index + 1], "id-720+ba");
        assert!(eligible(&cfg));
        assert!(!eligible(&TaskConfig {
            embed_metadata: true,
            ..cfg.clone()
        }));
        assert!(!eligible(&TaskConfig {
            preset: Preset::AudioMp3,
            ..cfg
        }));
        assert!(source_from_info(
            &json!({"protocol":"m3u8_native","url":"https://test/index.m3u8","has_drm":true})
        )
        .is_none());
    }

    #[test]
    fn direct_quality_caps_drop_only_known_higher_resolutions() {
        for (preset, limit) in [(Preset::Limit720, 720), (Preset::Limit1080, 1080)] {
            let cfg = TaskConfig {
                preset,
                ..Default::default()
            };
            let args = build_args(
                &cfg,
                &direct_source("https://test/master.m3u8"),
                Path::new("out/video"),
                Path::new("tmp"),
            )
            .unwrap();
            let i = args.iter().position(|s| s == "--drop-video").unwrap();
            let regex = args[i + 1]
                .strip_prefix("res=")
                .unwrap()
                .strip_suffix(":for=all")
                .unwrap();
            let re = regex::Regex::new(regex).unwrap();
            for height in [90, 720, 721, 1080, 1081, 1440, 2160, 10000] {
                assert_eq!(re.is_match(&format!("1920x{height}")), height > limit);
            }
        }
        assert!(safe_name(&"视频".repeat(100)).len() <= 160);
    }
}
