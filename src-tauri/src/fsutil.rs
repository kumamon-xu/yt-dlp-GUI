//! Atomic file writes for settings.json / queue.json.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

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

struct FileGate {
    mu: Mutex<()>,
    rev: AtomicU64,
}

fn gates() -> &'static Mutex<HashMap<PathBuf, Arc<FileGate>>> {
    static G: OnceLock<Mutex<HashMap<PathBuf, Arc<FileGate>>>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(HashMap::new()))
}

fn gate_for(path: &Path) -> Arc<FileGate> {
    let key = path.to_path_buf();
    let mut m = gates().lock().unwrap();
    m.entry(key)
        .or_insert_with(|| {
            Arc::new(FileGate {
                mu: Mutex::new(()),
                rev: AtomicU64::new(0),
            })
        })
        .clone()
}

#[allow(dead_code)]
pub fn current_rev(path: &Path) -> u64 {
    gate_for(path).rev.load(Ordering::SeqCst)
}

fn unique_tmp(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    parent.join(format!(".{name}.{}.{:x}.tmp", std::process::id(), n))
}

fn atomic_write_unlocked(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = unique_tmp(path);
    let write_res = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(data).map_err(|e| e.to_string())?;
        f.flush().map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        #[cfg(windows)]
        {
            winreplace::replace(&tmp, path)?;
        }
        #[cfg(not(windows))]
        {
            fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }
    write_res
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    let g = gate_for(path);
    let _lk = g.mu.lock().unwrap();
    atomic_write_unlocked(path, data)?;
    g.rev.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// Commit only if no successful write happened since `expected_rev`.
#[allow(dead_code)]
pub fn atomic_write_if_rev(path: &Path, data: &[u8], expected_rev: u64) -> Result<(), String> {
    let g = gate_for(path);
    let _lk = g.mu.lock().unwrap();
    if g.rev.load(Ordering::SeqCst) != expected_rev {
        return Err("stale revision".into());
    }
    atomic_write_unlocked(path, data)?;
    g.rev.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    atomic_write(path, json.as_bytes())
}

/// Snapshot/build payload while holding the per-path write lock, then replace the file.
pub fn commit_with(
    path: &Path,
    make: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let g = gate_for(path);
    let _lk = g.mu.lock().unwrap();
    let data = make()?;
    atomic_write_unlocked(path, &data)?;
    g.rev.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

#[allow(dead_code)]
pub fn atomic_write_json_if_rev<T: serde::Serialize>(
    path: &Path,
    value: &T,
    expected_rev: u64,
) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    atomic_write_if_rev(path, json.as_bytes(), expected_rev)
}

pub fn ensure_writable_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("无法创建输出目录 {}: {e}", dir.display()))?;
    let meta = fs::metadata(dir).map_err(|e| format!("无法访问输出目录 {}: {e}", dir.display()))?;
    if !meta.is_dir() {
        return Err(format!("输出路径不是目录: {}", dir.display()));
    }
    let probe = dir.join(format!(".ytdlp-wtest-{}", std::process::id()));
    fs::write(&probe, b"ok").map_err(|e| format!("输出目录不可写 {}: {e}", dir.display()))?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn tmp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ytdlp-atomic-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn atomic_write_leaves_valid_json() {
        let path = tmp_path("settings.json");
        let v = json!({"defaultPreset": "mp4", "n": 2});
        atomic_write_json(&path, &v).unwrap();
        let got: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got, v);
        atomic_write_json(&path, &json!({"defaultPreset": "best"})).unwrap();
        let got: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got["defaultPreset"], "best");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn concurrent_writers_leave_one_complete_json() {
        let path = tmp_path("queue.json");
        atomic_write_json(&path, &json!({"n": 0})).unwrap();
        let path = Arc::new(path);
        let mut hs = vec![];
        for i in 0..24 {
            let path = Arc::clone(&path);
            hs.push(std::thread::spawn(move || {
                atomic_write_json(&path, &json!({"n": i, "payload": "x".repeat(64)})).unwrap();
            }));
        }
        for h in hs {
            h.join().unwrap();
        }
        let got: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&*path).unwrap()).unwrap();
        assert!(got.get("n").and_then(|v| v.as_u64()).is_some(), "{got}");
        assert_eq!(got["payload"].as_str().unwrap().len(), 64);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn stale_revision_does_not_overwrite() {
        let path = tmp_path("rev.json");
        let r0 = current_rev(&path);
        atomic_write_json_if_rev(&path, &json!({"v": 1}), r0).unwrap();
        let r1 = current_rev(&path);
        assert_ne!(r1, r0);
        atomic_write_json(&path, &json!({"v": 2})).unwrap();
        let err = atomic_write_json_if_rev(&path, &json!({"v": "old"}), r1);
        assert!(err.is_err(), "{err:?}");
        let got: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got["v"], 2);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn write_to_missing_parent_of_file_as_dir_fails() {
        let dir = std::env::temp_dir().join(format!(
            "ytdlp-nowrite-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("blocked");
        std::fs::write(&blocker, b"not-a-dir").unwrap();
        let dest = blocker.join("settings.json");
        let err = atomic_write_json(&dest, &json!({"a": 1}));
        assert!(err.is_err(), "{err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_writable_dir_rejects_file() {
        let dir = std::env::temp_dir().join(format!(
            "ytdlp-ens-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("file");
        std::fs::write(&f, b"x").unwrap();
        assert!(ensure_writable_dir(&f).is_err());
        ensure_writable_dir(&dir.join("sub")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
