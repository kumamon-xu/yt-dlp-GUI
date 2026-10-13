//! Engine / ffmpeg lookup: override → managed → bundled → dev code/ → PATH.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolSource {
    Override,
    Managed,
    Bundled,
    Path,
}

#[derive(Debug, Clone, Default)]
pub struct ToolLookup {
    pub resource_dir: Option<PathBuf>,
    pub managed_dir: Option<PathBuf>,
    pub dev_code_dir: Option<PathBuf>,
    pub allow_path: bool,
}

pub fn bin_name(base: &str) -> String {
    if cfg!(windows) && !base.ends_with(".exe") {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

pub fn is_tool_file(p: &Path) -> bool {
    if !p.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn tool_not_executable_msg(p: &Path) -> String {
    format!("引擎不可执行：{}", p.display())
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if is_tool_file(&p) {
            return Some(p);
        }
    }
    None
}

/// Ordered lookup. Does not chmod. Unix non-executable files are skipped
/// (treated as missing) except override, which errors.
pub fn locate_tool(
    base: &str,
    override_path: Option<&str>,
    lookup: &ToolLookup,
) -> Result<(PathBuf, ToolSource), String> {
    let name = bin_name(base);
    if let Some(p) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        let pb = PathBuf::from(p);
        if !pb.is_file() {
            return Err(format!("指定路径不存在：{}", pb.display()));
        }
        if !is_tool_file(&pb) {
            return Err(tool_not_executable_msg(&pb));
        }
        return Ok((pb, ToolSource::Override));
    }

    if let Some(dir) = &lookup.managed_dir {
        let p = dir.join(&name);
        if is_tool_file(&p) {
            return Ok((p, ToolSource::Managed));
        }
    }

    if let Some(dir) = &lookup.resource_dir {
        let p = dir.join("code").join(&name);
        if is_tool_file(&p) {
            return Ok((p, ToolSource::Bundled));
        }
        #[cfg(target_os = "linux")]
        if base == "N_m3u8DL-RE" && p.with_extension("gz").is_file() {
            let managed = lookup
                .managed_dir
                .as_deref()
                .ok_or_else(|| "无法获取 RE 的本地缓存目录".to_string())?;
            return unpack_bundled_gzip(&p, managed).map(|path| (path, ToolSource::Bundled));
        }
    }

    if let Some(dir) = &lookup.dev_code_dir {
        let p = dir.join(&name);
        if is_tool_file(&p) {
            return Ok((p, ToolSource::Bundled));
        }
    }

    if lookup.allow_path {
        if let Some(p) = which_on_path(&name) {
            return Ok((p, ToolSource::Path));
        }
    }

    Err(format!(
        "未找到 {base}：请将 {name} 放到安装资源 code/ 或在设置中指定路径"
    ))
}

#[cfg(any(target_os = "linux", test))]
fn unpack_bundled_gzip(binary: &Path, managed: &Path) -> Result<PathBuf, String> {
    use std::io::Read;
    let expected = std::fs::read_to_string(binary.with_extension("sha256"))
        .map_err(|e| format!("无法读取 RE 校验文件：{e}"))?
        .trim()
        .to_ascii_lowercase();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("RE 校验文件无效".into());
    }
    let name = binary.file_name().ok_or("RE 文件名无效")?;
    let target = managed.join(format!("bundled-{expected}")).join(name);
    if is_tool_file(&target) && check_hash(&target, &expected).is_ok() {
        return Ok(target);
    }
    let file = std::fs::File::open(binary.with_extension("gz")).map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    flate2::read::GzDecoder::new(file)
        .take(128 * 1024 * 1024)
        .read_to_end(&mut data)
        .map_err(|e| format!("无法解压 RE：{e}"))?;
    let mut sha = Sha256Wrap::new();
    sha.update(&data);
    if sha.finalize_hex() != expected {
        return Err("RE 解压内容的 SHA-256 不匹配".into());
    }
    std::fs::create_dir_all(target.parent().ok_or("RE 缓存路径无效")?)
        .map_err(|e| e.to_string())?;
    crate::fsutil::atomic_write(&target, &data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(target)
}

#[allow(dead_code)]
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256Wrap::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

#[allow(dead_code)]
pub fn check_hash(path: &Path, expected_hex: &str) -> Result<(), String> {
    let got = sha256_file(path)?;
    let exp = expected_hex.trim().to_ascii_lowercase();
    if got != exp {
        return Err(format!(
            "hash mismatch for {}: expected {exp}, got {got}",
            path.display()
        ));
    }
    Ok(())
}

/// Minimal SHA-256 so tests don't need extra crates beyond what we already have.
/// Prefer sha2 if present in lockfile — check Cargo.toml.
#[allow(dead_code)]
struct Sha256Wrap {
    inner: sha2::Sha256,
}

#[allow(dead_code)]
impl Sha256Wrap {
    fn new() -> Self {
        use sha2::Digest;
        Self {
            inner: sha2::Sha256::new(),
        }
    }
    fn update(&mut self, data: &[u8]) {
        use sha2::Digest;
        self.inner.update(data);
    }
    fn finalize_hex(self) -> String {
        use sha2::Digest;
        format!("{:x}", self.inner.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("ytdlp-locate-{}", std::process::id()));
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = p.join(n.to_string());
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn bundled_gzip_preserves_bytes_repairs_cache_and_rejects_corruption() {
        let root = tmp_dir();
        let binary = root.join("code/N_m3u8DL-RE");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        let data = b"#!/bin/sh\nprintf 'RE test version\\n'\n";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        std::fs::write(binary.with_extension("gz"), encoder.finish().unwrap()).unwrap();
        let mut sha = Sha256Wrap::new();
        sha.update(data);
        let expected = sha.finalize_hex();
        std::fs::write(binary.with_extension("sha256"), &expected).unwrap();
        let managed = root.join("managed");
        let cached = unpack_bundled_gzip(&binary, &managed).unwrap();
        assert_eq!(std::fs::read(&cached).unwrap(), data);
        assert!(is_tool_file(&cached));
        assert_eq!(unpack_bundled_gzip(&binary, &managed).unwrap(), cached);
        std::fs::write(&cached, b"corrupted by a packager").unwrap();
        assert_eq!(unpack_bundled_gzip(&binary, &managed).unwrap(), cached);
        assert_eq!(std::fs::read(&cached).unwrap(), data);
        #[cfg(target_os = "linux")]
        {
            let (located, source) = locate_tool(
                "N_m3u8DL-RE",
                None,
                &ToolLookup {
                    resource_dir: Some(root.clone()),
                    managed_dir: Some(managed.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(located, cached);
            assert_eq!(source, ToolSource::Bundled);
        }
        std::fs::write(binary.with_extension("sha256"), "0".repeat(64)).unwrap();
        assert!(unpack_bundled_gzip(&binary, &managed).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    fn touch_exec(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(b"fake-engine").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = p.metadata().unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&p, perms).unwrap();
        }
        p
    }

    #[test]
    fn engines_lock_pins_sha256_not_latest() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engines.lock.json");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).expect("engines.lock.json")).unwrap();
        let url = v["yt-dlp"]["windows-x64"]["url"].as_str().unwrap();
        let sha = v["yt-dlp"]["windows-x64"]["sha256"].as_str().unwrap();
        assert!(!url.contains("/latest/"), "{url}");
        assert_eq!(sha.len(), 64);
        assert_eq!(
            sha,
            "66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a"
        );
        fn walk<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a str>) {
            match v {
                serde_json::Value::String(s) => out.push(s),
                serde_json::Value::Object(m) => {
                    for x in m.values() {
                        walk(x, out);
                    }
                }
                serde_json::Value::Array(a) => {
                    for x in a {
                        walk(x, out);
                    }
                }
                _ => {}
            }
        }
        let mut strings = Vec::new();
        walk(&v, &mut strings);
        for s in strings {
            assert!(!s.contains("/latest/"), "lock must not use latest: {s}");
        }
    }

    #[test]
    fn release_workflow_never_deletes_stable() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows");
        let release = std::fs::read_to_string(root.join("release.yml")).expect("release.yml");
        let reusable =
            std::fs::read_to_string(root.join("build-release.yml")).expect("build-release.yml");
        let dry_run =
            std::fs::read_to_string(root.join("stable-dry-run.yml")).expect("stable-dry-run.yml");
        let t = format!("{release}\n{reusable}");
        assert!(
            !t.contains("gh release delete v"),
            "must never delete vX.Y.Z"
        );
        assert!(
            !t.contains("gh release delete nightly"),
            "must not delete nightly before a successful matrix"
        );
        assert!(release.contains("ref_name!=expect"));
        assert!(release.contains("needs: [verify-version, build]"));
        assert!(release.contains("prepare-release-assets.py"));
        assert!(release.contains("--draft=false"));
        assert!(release.contains("clear_release_assets"));
        assert!(release.contains("gh release delete-asset \"$RELEASE_TAG\" \"$asset\" --yes"));
        assert!(release.contains("release assets were not fully cleared"));
        assert!(release.contains("gh release upload \"$RELEASE_TAG\" publish/*"));
        assert!(!release.contains("publish/* --clobber"));
        assert!(reusable.contains("pnpm test"));
        assert!(reusable.contains("clippy"));
        assert!(reusable.contains("-Lock") && reusable.contains("--lock"));
        assert!(reusable.contains("check-bundled-engines"));
        assert!(reusable.contains("write-engines-manifest"));
        assert!(reusable.contains("engines-manifest-${{ matrix.slug }}"));
        assert!(
            reusable.contains("cargo test --manifest-path src-tauri/Cargo.toml --lib"),
            "Windows bin tests double-link VERSION resources"
        );
        assert!(
            !t.contains("engines-manifest.json --clobber || true"),
            "manifest upload must fail the job"
        );
        assert!(dry_run.contains("fetch_mode: lock"));
        assert!(!dry_run.contains("gh release"));
        let py = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/write-engines-manifest.py"),
        )
        .unwrap();
        assert!(py.contains("lock_url"));
        assert!(py.contains("latest_url"));
        let fetch = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/fetch-engines.ps1"),
        )
        .unwrap();
        assert!(fetch.contains("Get-Sha256"));
        assert!(fetch.contains("-Lock"));
        let ci = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/ci.yml"),
        )
        .unwrap();
        assert!(ci.contains("pnpm test"));
        assert!(ci.contains("clippy"));
        assert!(ci.contains("fmt"));
        assert!(ci.contains("windows-latest"));
        assert!(ci.contains("macos-latest"));
        let build =
            std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("build.rs"))
                .unwrap();
        assert!(
            !build.contains("rustc-link-arg="),
            "linking resource.lib via rustc-link-arg double-links the Windows app bin"
        );
        assert!(build.contains("rustc-link-search=native"));
        let sh = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/check-bundled-engines.sh"),
        )
        .unwrap();
        assert!(sh.contains("found_yt"), "unix smoke must require yt-dlp");
        assert!(sh.contains("found_ff"), "unix smoke must require ffmpeg");
        assert!(
            sh.contains("found_yt") && sh.contains("found_ff") && sh.contains("-eq 0"),
            "both engines required"
        );
        let ps = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/check-bundled-engines.ps1"),
        )
        .unwrap();
        assert!(
            ps.contains("LASTEXITCODE"),
            "windows smoke must fail closed on --version"
        );
    }

    #[test]
    fn bin_name_matches_platform() {
        #[cfg(windows)]
        {
            assert_eq!(bin_name("yt-dlp"), "yt-dlp.exe");
            assert_eq!(bin_name("ffmpeg"), "ffmpeg.exe");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(bin_name("yt-dlp"), "yt-dlp");
            assert_eq!(bin_name("ffmpeg"), "ffmpeg");
        }
    }

    #[test]
    fn override_wins_over_bundled() {
        let root = tmp_dir();
        let bundled = root.join("res");
        std::fs::create_dir_all(bundled.join("code")).unwrap();
        touch_exec(&bundled.join("code"), &bin_name("yt-dlp"));
        let ov = touch_exec(&root, &bin_name("custom"));
        let lookup = ToolLookup {
            resource_dir: Some(bundled),
            allow_path: false,
            ..Default::default()
        };
        let (p, src) = locate_tool("yt-dlp", Some(ov.to_str().unwrap()), &lookup).unwrap();
        assert_eq!(src, ToolSource::Override);
        assert_eq!(p, ov);
    }

    #[test]
    fn bundled_code_layout_beats_path() {
        let root = tmp_dir();
        let bundled = root.join("res");
        std::fs::create_dir_all(bundled.join("code")).unwrap();
        let want = touch_exec(&bundled.join("code"), &bin_name("ffmpeg"));
        let lookup = ToolLookup {
            resource_dir: Some(bundled),
            allow_path: true,
            ..Default::default()
        };
        let (p, src) = locate_tool("ffmpeg", None, &lookup).unwrap();
        assert_eq!(src, ToolSource::Bundled);
        assert_eq!(p, want);
    }

    #[test]
    fn missing_bundled_does_not_invent_path_when_disabled() {
        let lookup = ToolLookup {
            resource_dir: Some(tmp_dir().join("empty-res")),
            allow_path: false,
            ..Default::default()
        };
        assert!(locate_tool("yt-dlp", None, &lookup).is_err());
    }

    #[test]
    fn managed_beats_bundled() {
        let root = tmp_dir();
        let bundled = root.join("res");
        std::fs::create_dir_all(bundled.join("code")).unwrap();
        touch_exec(&bundled.join("code"), &bin_name("yt-dlp"));
        let managed = root.join("engines");
        std::fs::create_dir_all(&managed).unwrap();
        let want = touch_exec(&managed, &bin_name("yt-dlp"));
        let lookup = ToolLookup {
            resource_dir: Some(bundled),
            managed_dir: Some(managed),
            allow_path: false,
            ..Default::default()
        };
        let (p, src) = locate_tool("yt-dlp", None, &lookup).unwrap();
        assert_eq!(src, ToolSource::Managed);
        assert_eq!(p, want);
    }

    #[test]
    fn sha256_roundtrip_and_mismatch() {
        let dir = tmp_dir();
        let p = dir.join("blob.bin");
        std::fs::write(&p, b"hello-lock").unwrap();
        let h = sha256_file(&p).unwrap();
        // Independently: SHA256("hello-lock") via .NET SHA256
        assert_eq!(
            h,
            "70b02949aece3cd4114d3d7640b806a315ff28efd3579f1e03342d65a02fa090"
        );
        check_hash(&p, &h).unwrap();
        assert!(check_hash(&p, "deadbeef").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unix_nonexecutable_is_rejected_not_chmoded() {
        let dir = tmp_dir();
        let p = dir.join("yt-dlp");
        std::fs::write(&p, b"no-exec").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = p.metadata().unwrap().permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&p, perms).unwrap();
        let lookup = ToolLookup {
            managed_dir: Some(dir.clone()),
            allow_path: false,
            ..Default::default()
        };
        assert!(locate_tool("yt-dlp", None, &lookup).is_err());
        let mode = p.metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "must not chmod");
    }
}
