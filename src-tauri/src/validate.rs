//! Shared validation for settings and task options (Rust is source of truth).

use crate::config::GlobalSettings;

pub fn validate_concurrent_fragments(n: u32) -> Result<u32, String> {
    if (1..=32).contains(&n) {
        Ok(n)
    } else {
        Err("concurrentFragments must be 1..=32".into())
    }
}

pub fn validate_max_concurrent_tasks(n: u32) -> Result<u32, String> {
    if (1..=8).contains(&n) {
        Ok(n)
    } else {
        Err("maxConcurrentTasks must be 1..=8".into())
    }
}

pub fn validate_limit_rate(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let re = regex_lite_rate(s);
    if re {
        Ok(Some(s.to_string()))
    } else {
        Err("limitRate must look like 500K, 2M, or 10M".into())
    }
}

fn regex_lite_rate(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() {
        return false;
    }
    let num = match b.last().copied() {
        Some(c) if c == b'K' || c == b'M' || c == b'G' => &s[..s.len() - 1],
        _ => s,
    };
    if num.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    for (i, c) in num.chars().enumerate() {
        if c == '.' {
            if seen_dot || i == 0 || i + 1 == num.chars().count() {
                return false;
            }
            seen_dot = true;
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    true
}

pub fn validate_proxy(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let ok = s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("socks4://")
        || s.starts_with("socks5://")
        || s.starts_with("socks5h://");
    if ok {
        Ok(Some(s.to_string()))
    } else {
        Err("proxy must start with http://, https://, socks4://, socks5://, or socks5h://".into())
    }
}

pub fn validate_playlist_items(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let ok = s.split(',').all(|part| {
        let part = part.trim();
        if part.is_empty() {
            return false;
        }
        if let Some((a, b)) = part.split_once('-') {
            return a.chars().all(|c| c.is_ascii_digit())
                && !a.is_empty()
                && b.chars().all(|c| c.is_ascii_digit())
                && !b.is_empty();
        }
        part.chars().all(|c| c.is_ascii_digit())
    });
    if ok {
        Ok(Some(s.to_string()))
    } else {
        Err("playlistItems must look like 1,2,5-10".into())
    }
}

pub fn validate_settings(s: &GlobalSettings) -> Result<(), String> {
    validate_concurrent_fragments(s.concurrent_fragments)?;
    validate_max_concurrent_tasks(s.max_concurrent_tasks)?;
    validate_limit_rate(s.limit_rate.as_deref())?;
    validate_proxy(s.proxy.as_deref())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_range() {
        assert!(validate_concurrent_fragments(1).is_ok());
        assert!(validate_concurrent_fragments(32).is_ok());
        assert!(validate_concurrent_fragments(0).is_err());
        assert!(validate_concurrent_fragments(999999).is_err());
    }

    #[test]
    fn rate_and_proxy_and_playlist() {
        assert_eq!(
            validate_limit_rate(Some("500K")).unwrap().as_deref(),
            Some("500K")
        );
        assert_eq!(
            validate_limit_rate(Some("2M")).unwrap().as_deref(),
            Some("2M")
        );
        assert_eq!(
            validate_limit_rate(Some("1.5M")).unwrap().as_deref(),
            Some("1.5M")
        );
        assert!(validate_limit_rate(Some("fast")).is_err());
        assert!(validate_limit_rate(Some("2m")).is_err());
        assert!(validate_limit_rate(Some(".5M")).is_err());
        assert!(validate_proxy(Some("http://127.0.0.1:7890")).is_ok());
        assert!(validate_proxy(Some("socks5://127.0.0.1:1080")).is_ok());
        assert!(validate_proxy(Some("ftp://x")).is_err());
        assert!(validate_playlist_items(Some("1,2,5-10")).is_ok());
        assert!(validate_playlist_items(Some("1-")).is_err());
        assert!(validate_playlist_items(Some("abc")).is_err());
    }

    #[test]
    fn settings_rejects_bad_fragments() {
        let bad_n = GlobalSettings {
            concurrent_fragments: 0,
            ..Default::default()
        };
        assert!(validate_settings(&bad_n).is_err());
        let bad_proxy = GlobalSettings {
            proxy: Some("not-a-url".into()),
            ..Default::default()
        };
        assert!(validate_settings(&bad_proxy).is_err());
        let ok = GlobalSettings {
            proxy: Some("http://127.0.0.1:1".into()),
            ..Default::default()
        };
        assert!(validate_settings(&ok).is_ok());
    }
}
