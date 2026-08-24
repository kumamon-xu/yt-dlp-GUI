//! Redact URL userinfo for display / queue persistence.

/// `http://user:secret@host` → `http://user:***@host`
pub fn redact_userinfo(s: &str) -> String {
    let Some(scheme) = s.find("://") else {
        return s.to_string();
    };
    let rest = &s[scheme + 3..];
    let Some(at) = rest.find('@') else {
        return s.to_string();
    };
    let creds = &rest[..at];
    let Some(colon) = creds.find(':') else {
        return s.to_string();
    };
    let user = &creds[..colon];
    format!("{}{user}:***{}", &s[..scheme + 3], &rest[at..])
}

/// `http://user:secret@host` → `http://host` (no credentials).
pub fn strip_userinfo(s: &str) -> String {
    let Some(scheme) = s.find("://") else {
        return s.to_string();
    };
    let rest = &s[scheme + 3..];
    let Some(at) = rest.find('@') else {
        return s.to_string();
    };
    format!("{}{}", &s[..scheme + 3], &rest[at + 1..])
}

pub fn format_command_preview(engine: &str, args: &[String]) -> String {
    std::iter::once(crate::command::quote_arg(engine))
        .chain(
            args.iter()
                .map(|a| crate::command::quote_arg(&redact_userinfo(a))),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_password_keeps_user_and_host() {
        let s = "http://alice:secret@127.0.0.1:7890";
        let r = redact_userinfo(s);
        assert!(!r.contains("secret"), "{r}");
        assert!(r.contains("alice"), "{r}");
        assert!(r.contains("127.0.0.1:7890"), "{r}");
        assert_eq!(r, "http://alice:***@127.0.0.1:7890");
    }

    #[test]
    fn strip_removes_userinfo() {
        let s = "http://alice:secret@proxy.example.com";
        let t = strip_userinfo(s);
        assert!(!t.contains("secret"));
        assert!(!t.contains("alice"));
        assert_eq!(t, "http://proxy.example.com");
    }

    #[test]
    fn preview_command_masks_proxy_but_raw_args_keep_secret() {
        let args = vec![
            "--proxy".into(),
            "http://alice:secret@127.0.0.1:7890".into(),
        ];
        let preview = format_command_preview("yt-dlp", &args);
        assert!(!preview.contains("secret"), "{preview}");
        assert!(args[1].contains("secret"));
    }
}
