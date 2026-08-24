//! Atomic file writes for settings.json / queue.json.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[cfg(windows)]
mod winreplace {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    pub fn replace(from: &Path, to: &Path) -> Result<(), String> {
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        fn wide(p: &Path) -> Vec<u16> {
            p.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }
        let src = wide(from);
        let dst = wide(to);
        let ok = unsafe {
            MoveFileExW(
                src.as_ptr(),
                dst.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            Err(format!(
                "MoveFileExW failed: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("file")
    ));

    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(data).map_err(|e| e.to_string())?;
        f.flush().map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }

    #[cfg(windows)]
    {
        winreplace::replace(&tmp, path)?;
    }
    #[cfg(not(windows))]
    {
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    atomic_write(path, json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn atomic_write_leaves_valid_json() {
        let dir = std::env::temp_dir().join(format!(
            "ytdlp-atomic-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let v = json!({"defaultPreset": "mp4", "n": 2});
        atomic_write_json(&path, &v).unwrap();
        let got: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got, v);
        atomic_write_json(&path, &json!({"defaultPreset": "best"})).unwrap();
        let got: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got["defaultPreset"], "best");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
