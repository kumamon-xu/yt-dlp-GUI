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

/// stderr → 用户可见中文（先匹配先命中）
pub fn friendly_error(stderr: &str) -> String {
    let t = stderr.trim();
    let lower = t.to_lowercase();
    let tail: String = t.lines().rev().take(3).collect::<Vec<_>>().join(" ");

    if lower.contains("age confirmation") || lower.contains("sign in to confirm your age") {
        "需要年龄验证：请导入浏览器 Cookies".into()
    } else if lower.contains("sign in to view full functionality")
        || (lower.contains("403") && lower.contains("bilibili"))
    {
        "未登录：高清需要 Cookies".into()
    } else if lower.contains("unable to load cookies") || lower.contains("unable to read cookies") {
        "请关闭目标浏览器后重试".into()
    } else if lower.contains("requested format is not available") {
        "所选清晰度不存在，请换一档".into()
    } else if lower.contains("ffmpeg") && lower.contains("not found") {
        "未检测到 ffmpeg，合并/音频不可用".into()
    } else if lower.contains("no video formats") {
        "无法解析或需要登录/地区限制".into()
    } else if lower.contains("unable to extract") {
        "无法解析或需要登录/地区限制".into()
    } else if lower.contains("private video") {
        "私密视频：需要登录且有权限".into()
    } else if lower.contains("unable to download") && lower.contains("proxy") {
        "代理连接失败".into()
    } else if lower.contains("timed out") || lower.contains("timeout") || lower.contains("socket.timeout") {
        "网络超时，可重试或配置代理".into()
    } else if lower.contains("404") || lower.contains("not found") || lower.contains("does not exist") {
        "链接不存在或已删除".into()
    } else {
        format!("失败：{tail}")
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
        let (_, _, _, _, _, title) =
            parse_progress("YDLP|finished|1|1|1|0|foo|bar").unwrap();
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
    fn template_has_estimate_fallback() {
        assert!(DOWNLOAD_TPL.contains("total_bytes,progress.total_bytes_estimate"));
        assert!(FILE_PRINT.starts_with("post_process:YDLPFILE|"));
    }
}
