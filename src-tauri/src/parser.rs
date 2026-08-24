//! 进度协议常量、进度行解析、stderr 友好错误映射。
//! 本文件是唯一来源：禁止在其它模块复制前缀/模板。

/// 进度行前缀：`YDLP|status|downloaded|total|speed|eta|title`
pub const PROGRESS_PREFIX: &str = "YDLP|";
/// 完成行前缀：`YDLPFILE|<最终文件路径>`
pub const FILE_PREFIX: &str = "YDLPFILE|";
/// 下载进度模板（total_bytes 缺失时回退 estimate）
pub const DOWNLOAD_TPL: &str = "download:YDLP|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes,progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.title)s";
/// 后处理完成时打印最终文件路径
pub const FILE_PRINT: &str = "post_process:YDLPFILE|%(filepath)s";

fn parse_num<T: std::str::FromStr + Default>(s: &str) -> T {
    s.trim().parse().unwrap_or_default()
}

/// 解析 `YDLP|status|dl|total|speed|eta|title`
pub fn parse_progress(line: &str) -> Option<(String, u64, u64, f64, f64, String)> {
    let rest = line.strip_prefix(PROGRESS_PREFIX)?;
    let parts: Vec<&str> = rest.splitn(6, '|').collect();
    if parts.len() < 5 {
        return None;
    }
    Some((
        parts[0].to_string(),
        parse_num(parts[1]),
        parse_num(parts[2]),
        parse_num(parts[3]),
        parse_num(parts[4]),
        parts.get(5).map(|s| s.to_string()).unwrap_or_default(),
    ))
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub code: &'static str,
    pub title: String,
    #[allow(dead_code)]
    pub detail: String,
}

pub fn friendly_error(stderr: &str) -> String {
    classify_error(stderr).title
}

/// stderr → structured error. 404 alone is not always "video deleted".
pub fn classify_error(stderr: &str) -> AppError {
    let t = stderr.trim();
    let lower = t.to_lowercase();
    let tail: String = t.lines().rev().take(3).collect::<Vec<_>>().join(" ");

    let (code, title) = if lower.contains("age confirmation")
        || lower.contains("sign in to confirm your age")
    {
        ("AUTH_REQUIRED", "需要年龄验证：请导入浏览器 Cookies")
    } else if lower.contains("sign in to view full functionality")
        || (lower.contains("403") && lower.contains("bilibili"))
    {
        ("AUTH_REQUIRED", "未登录：高清需要 Cookies")
    } else if lower.contains("unable to load cookies") || lower.contains("unable to read cookies") {
        ("AUTH_REQUIRED", "请关闭目标浏览器后重试")
    } else if lower.contains("requested format is not available") {
        ("FORMAT_UNAVAILABLE", "所选清晰度不存在，请换一档")
    } else if lower.contains("ffmpeg") && lower.contains("not found") {
        ("FFMPEG_MISSING", "未检测到 ffmpeg，合并/音频不可用")
    } else if lower.contains("no video formats") {
        ("FORMAT_UNAVAILABLE", "无法解析或需要登录/地区限制")
    } else if lower.contains("unable to extract") {
        ("PROCESS_FAILED", "无法解析或需要登录/地区限制")
    } else if lower.contains("private video") {
        ("AUTH_REQUIRED", "私密视频：需要登录且有权限")
    } else if lower.contains("unable to download") && lower.contains("proxy") {
        ("NETWORK_TIMEOUT", "代理连接失败")
    } else if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("socket.timeout")
    {
        ("NETWORK_TIMEOUT", "网络超时，可重试或配置代理")
    } else if lower.contains("http error 404")
        && (lower.contains("webpage") || lower.contains("video"))
    {
        ("RESOURCE_NOT_FOUND", "链接不存在或已删除")
    } else if lower.contains("404") {
        ("PROCESS_FAILED", "下载过程出现 404（不一定是链接失效）")
    } else {
        ("UNKNOWN", "任务失败")
    };
    AppError {
        code,
        title: if code == "UNKNOWN" && !tail.is_empty() {
            format!("失败：{tail}")
        } else {
            title.into()
        },
        detail: tail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progress_line() {
        let (st, dl, total, speed, eta, title) =
            parse_progress("YDLP|downloading|130048|788493|5701601.85|0|mov_bbb").unwrap();
        assert_eq!(st, "downloading");
        assert_eq!(dl, 130048);
        assert_eq!(total, 788493);
        assert!(speed > 0.0);
        assert_eq!(eta, 0.0);
        assert_eq!(title, "mov_bbb");
    }

    #[test]
    fn parse_na_as_zero() {
        let (_, _, total, speed, eta, _) =
            parse_progress("YDLP|downloading|1024|NA|NA|NA|t").unwrap();
        assert_eq!(total, 0);
        assert_eq!(speed, 0.0);
        assert_eq!(eta, 0.0);
    }

    #[test]
    fn parse_title_with_pipe() {
        let (_, _, _, _, _, title) = parse_progress("YDLP|finished|1|1|1|0|foo|bar").unwrap();
        assert_eq!(title, "foo|bar");
    }

    #[test]
    fn maps_age_gate() {
        assert!(friendly_error("Sign in to confirm your age").contains("年龄"));
    }

    #[test]
    fn maps_cookies_locked() {
        assert!(friendly_error("Unable to load cookies: browser is locked").contains("关闭"));
    }

    #[test]
    fn maps_ffmpeg_not_found() {
        assert!(friendly_error("ERROR: ffmpeg not found").contains("ffmpeg"));
    }

    #[test]
    fn maps_missing_format() {
        assert!(friendly_error("Requested format is not available").contains("清晰度"));
    }

    #[test]
    fn maps_bilibili_403() {
        assert!(friendly_error("HTTP Error 403: Forbidden [Bilibili]").contains("Cookies"));
    }

    #[test]
    fn fallback_is_neutral() {
        let s = friendly_error("line1\nweird unknown boom");
        assert!(s.starts_with("失败："), "{s}");
        assert!(!s.contains("解析失败"));
    }

    #[test]
    fn bare_404_is_not_deleted_video() {
        let e = classify_error("WARNING: fragment 3: HTTP Error 404");
        assert_eq!(e.code, "PROCESS_FAILED");
        assert!(!e.title.contains("链接不存在"));
        assert!(!e.detail.is_empty());
        let e = classify_error("ERROR: [youtube] abc: HTTP Error 404: Not Found webpage");
        assert_eq!(e.code, "RESOURCE_NOT_FOUND");
        assert!(e.detail.to_lowercase().contains("404"));
    }

    #[test]
    fn template_has_estimate_fallback() {
        assert!(DOWNLOAD_TPL.contains("total_bytes,progress.total_bytes_estimate"));
        assert!(FILE_PRINT.starts_with("post_process:YDLPFILE|"));
    }
}
