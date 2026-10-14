//! Bounded VOD startup checks. RecoveryPlan is also the routing seam for a future
//! per-request local HLS service; startup checks do not provide mid-run failover.

use crate::command::{Preset, TaskConfig};
use crate::m3u8::HlsSource;
use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use reqwest::header::{HeaderName, HeaderValue, CONTENT_RANGE, LOCATION, RANGE};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const SAMPLE_LIMIT: usize = 64 * 1024;
const MANIFEST_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MirrorRule {
    pub from_origin: String,
    pub to_origin: String,
    pub path_prefix: String,
}

fn origin(raw: &str) -> Result<Url, String> {
    let u = Url::parse(raw).map_err(|_| "备用映射必须填写完整的 http(s) 源地址".to_string())?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.path() != "/"
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("备用映射源地址只能包含协议、域名和端口，不能包含认证、路径或参数".into());
    }
    Ok(u)
}

pub fn validate_rules(rules: &[MirrorRule]) -> Result<(), String> {
    if rules.len() > 32 {
        return Err("可信备用映射最多 32 条".into());
    }
    for r in rules {
        let from = origin(&r.from_origin)?;
        let to = origin(&r.to_origin)?;
        if from.origin() == to.origin() || (from.scheme() == "https" && to.scheme() != "https") {
            return Err("备用地址必须不同于原地址，且不能从 HTTPS 降级到 HTTP".into());
        }
        if !r.path_prefix.starts_with('/')
            || !r.path_prefix.ends_with('/')
            || r.path_prefix.contains(['?', '#', '\\', '\r', '\n'])
            || r.path_prefix.split('/').any(|s| matches!(s, "." | ".."))
        {
            return Err("备用映射路径必须以 / 开头和结尾，不能包含参数或上级路径".into());
        }
    }
    Ok(())
}

impl MirrorRule {
    pub fn candidate(&self, resource: &Url) -> Option<Url> {
        let from = origin(&self.from_origin).ok()?;
        let to = origin(&self.to_origin).ok()?;
        if resource.origin() != from.origin() || !resource.path().starts_with(&self.path_prefix) {
            return None;
        }
        // Replace the origin only; signed path/query octets and their order stay intact.
        let mut u = resource.clone();
        u.set_scheme(to.scheme()).ok()?;
        u.set_host(to.host_str()).ok()?;
        u.set_port(to.port()).ok()?;
        Some(u)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecoveryPlan {
    pub routes: Vec<MirrorRule>,
}

impl RecoveryPlan {
    pub fn route(&self, url: &Url) -> Url {
        self.routes
            .iter()
            .find_map(|r| r.candidate(url))
            .unwrap_or_else(|| url.clone())
    }
}

#[derive(Debug)]
struct TemporaryManifests(PathBuf);
impl Drop for TemporaryManifests {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, Clone)]
pub struct PreparedInput {
    pub input: String,
    pub plan: RecoveryPlan,
    pub messages: Vec<String>,
    _files: Option<Arc<TemporaryManifests>>,
}

pub struct RequestContext {
    client: reqwest::Client,
    source: Url,
    headers: BTreeMap<String, String>,
    cookies: Option<String>,
}

fn network_error(e: reqwest::Error) -> ProbeError {
    // Do not retain URLs, signed parameters, credentials or response bodies.
    let kind = if e.is_timeout() {
        "连接超时"
    } else {
        "TLS/网络连接失败"
    };
    ProbeError {
        reason: kind.into(),
        recoverable: true,
    }
}

#[derive(Debug)]
pub struct ProbeError {
    pub reason: String,
    pub recoverable: bool,
}
impl ProbeError {
    fn invalid(reason: &str) -> Self {
        Self {
            reason: reason.into(),
            recoverable: false,
        }
    }
    fn bad_media() -> Self {
        Self {
            reason: "返回内容不是有效的媒体分片".into(),
            recoverable: true,
        }
    }
}

impl RequestContext {
    pub fn new(
        cfg: &TaskConfig,
        source: &HlsSource,
        cookie_text: Option<String>,
    ) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12));
        if let Some(proxy) = cfg.proxy.as_ref().filter(|p| !p.is_empty()) {
            builder = builder
                .proxy(reqwest::Proxy::all(proxy).map_err(|_| "无效的检查代理".to_string())?);
        }
        Ok(Self {
            client: builder
                .build()
                .map_err(|_| "无法初始化 HLS 网络检查".to_string())?,
            source: http_url(&source.url)?,
            headers: source.headers.clone(),
            cookies: cookie_text,
        })
    }

    fn cookie(&self, url: &Url) -> Option<String> {
        if let Some(text) = &self.cookies {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let c = crate::m3u8::cookie_header(text, url.as_str(), now);
            return (!c.is_empty()).then_some(c);
        }
        (url.origin() == self.source.origin())
            .then(|| {
                self.headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("cookie"))
                    .map(|(_, v)| v.clone())
            })
            .flatten()
    }

    fn allows_engine_headers(&self, url: &Url) -> bool {
        // RE accepts one global header set. Until the request service exists,
        // reject switches that need different scoped credentials.
        self.headers.iter().all(|(k, v)| {
            if k.eq_ignore_ascii_case("cookie") {
                self.cookie(url).as_ref() == Some(v)
            } else if is_scoped_header(k) {
                url.origin() == self.source.origin()
            } else {
                true
            }
        }) && (self.cookie(url).is_none()
            || self.headers.iter().any(|(k, v)| {
                k.eq_ignore_ascii_case("cookie") && self.cookie(url).as_ref() == Some(v)
            }))
    }

    pub(crate) async fn get(
        &self,
        start: &Url,
        range: Option<(u64, u64)>,
        limit: usize,
        complete: bool,
    ) -> Result<(Url, Vec<u8>), ProbeError> {
        let mut url = start.clone();
        for _ in 0..6 {
            let mut request = self.client.get(url.clone());
            for (k, v) in &self.headers {
                if k.eq_ignore_ascii_case("cookie")
                    || k.eq_ignore_ascii_case("host")
                    || k.eq_ignore_ascii_case("range")
                    || k.eq_ignore_ascii_case("accept-encoding")
                {
                    continue;
                }
                if is_scoped_header(k) && url.origin() != self.source.origin() {
                    continue;
                }
                let name = HeaderName::from_bytes(k.as_bytes())
                    .map_err(|_| ProbeError::invalid("请求头无效"))?;
                let value =
                    HeaderValue::from_str(v).map_err(|_| ProbeError::invalid("请求头无效"))?;
                request = request.header(name, value);
            }
            if let Some(cookie) = self.cookie(&url) {
                request = request.header("Cookie", cookie);
            }
            request = request.header("Accept-Encoding", "identity");
            if let Some((offset, length)) = range {
                let end = offset
                    .checked_add(length - 1)
                    .ok_or_else(|| ProbeError::invalid("字节范围溢出"))?;
                request = request.header(RANGE, format!("bytes={offset}-{end}"));
            }
            let mut response = request.send().await.map_err(network_error)?;
            let status = response.status().as_u16();
            if [301, 302, 303, 307, 308].contains(&status) {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| ProbeError::invalid("重定向缺少目标地址"))?;
                let next = url
                    .join(location)
                    .map_err(|_| ProbeError::invalid("重定向地址无效"))?;
                http_url(next.as_str()).map_err(|_| ProbeError::invalid("重定向协议无效"))?;
                if url.scheme() == "https" && next.scheme() != "https" {
                    return Err(ProbeError::invalid("检查拒绝 HTTPS 降级重定向"));
                }
                if !self.allows_engine_headers(&next) {
                    return Err(ProbeError::invalid("重定向需要不同的认证头"));
                }
                url = next;
                continue;
            }
            if status != 200 && status != 206 {
                return Err(ProbeError {
                    reason: format!("HTTP {status}"),
                    recoverable: matches!(status, 404 | 410 | 500..=599),
                });
            }
            if status == 206 {
                let (expected_start, requested_length) = range.ok_or_else(ProbeError::bad_media)?;
                let content_range = response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let parsed = content_range
                    .strip_prefix("bytes ")
                    .and_then(|s| s.split_once('/'))
                    .and_then(|(span, total)| {
                        let (start, end) = span.split_once('-')?;
                        Some((
                            start.parse::<u64>().ok()?,
                            end.parse::<u64>().ok()?,
                            total.parse::<u64>().ok()?,
                        ))
                    });
                let Some((start, end, total)) = parsed else {
                    return Err(ProbeError::bad_media());
                };
                if start != expected_start
                    || end < start
                    || end >= total
                    || end != (expected_start + requested_length - 1).min(total - 1)
                {
                    return Err(ProbeError::bad_media());
                }
            } else if range.is_some_and(|r| r.0 != 0) {
                return Err(ProbeError {
                    reason: "服务器未按字节范围返回分片".into(),
                    recoverable: true,
                });
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(network_error)? {
                let remaining = limit.saturating_sub(bytes.len());
                if complete && chunk.len() > remaining {
                    return Err(ProbeError::invalid("响应超过检查大小上限"));
                }
                bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                if !complete && bytes.len() == limit {
                    break;
                }
            }
            if bytes.is_empty() {
                return Err(ProbeError::bad_media());
            }
            return Ok((url, bytes));
        }
        Err(ProbeError::invalid("重定向次数超限"))
    }
}

fn is_scoped_header(name: &str) -> bool {
    ![
        "user-agent",
        "referer",
        "accept",
        "accept-language",
        "accept-encoding",
        "origin",
        "sec-fetch-mode",
        "sec-fetch-site",
        "sec-fetch-dest",
    ]
    .contains(&name.to_ascii_lowercase().as_str())
}

fn http_url(raw: &str) -> Result<Url, String> {
    let u = Url::parse(raw).map_err(|_| "HLS 资源地址无效".to_string())?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err("HLS 检查仅支持无内嵌认证的 HTTP(S) 地址".into());
    }
    Ok(u)
}

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Segment,
    Init,
    Key,
}
#[derive(Clone, Debug)]
pub(crate) struct Resource {
    pub(crate) line: usize,
    pub(crate) url: Url,
    pub(crate) kind: Kind,
    pub(crate) range: Option<(u64, u64)>,
    pub(crate) crypto: Option<(String, [u8; 16])>,
}

fn attributes(line: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let text = line.split_once(':').map(|p| p.1).unwrap_or("");
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ',')))
    {
        if c == '"' {
            quoted = !quoted;
        }
        if c == ',' && !quoted {
            if let Some((k, v)) = text[start..i].split_once('=') {
                result.insert(k.trim().into(), v.trim().trim_matches('"').into());
            }
            start = i + 1;
        }
    }
    result
}

fn attr(line: &str, name: &str) -> Option<String> {
    attributes(line).remove(name)
}
fn replace_uri(line: &str, url: &str) -> Result<String, String> {
    if !line.starts_with('#') {
        return Ok(url.into());
    }
    let start = line
        .find("URI=\"")
        .ok_or_else(|| "清单 URI 属性无效".to_string())?
        + 5;
    let end = line[start..]
        .find('"')
        .ok_or_else(|| "清单 URI 引号无效".to_string())?
        + start;
    Ok(format!("{}{}{}", &line[..start], url, &line[end..]))
}

fn byte_range(raw: &str, implicit: Option<u64>) -> Result<(u64, u64), String> {
    let (length, offset) = raw
        .split_once('@')
        .map(|(n, o)| (n, Some(o)))
        .unwrap_or((raw, None));
    let length = length
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or("无效的 HLS 字节范围")?;
    let offset = match offset {
        Some(o) => o.parse::<u64>().map_err(|_| "无效的字节偏移")?,
        None => implicit.ok_or("缺少前一个同资源字节范围")?,
    };
    offset.checked_add(length).ok_or("字节范围溢出")?;
    Ok((offset, length))
}

fn media_resources(lines: &[String], base: &Url) -> Result<Vec<Resource>, String> {
    let mut resources = Vec::new();
    let mut sequence = 0u64;
    let mut key = None;
    let mut explicit_iv = None;
    let mut pending_range = None;
    let mut last_range: Option<(Url, u64)> = None;
    let mut duration_pending = false;
    for (index, line) in lines.iter().enumerate() {
        if let Some(raw) = line.strip_prefix("#EXTINF:") {
            let duration = raw
                .split(',')
                .next()
                .unwrap_or("")
                .parse::<f64>()
                .map_err(|_| "无效的分片时长")?;
            if !duration.is_finite() || duration <= 0. {
                return Err("无效的分片时长".into());
            }
            duration_pending = true;
        } else if let Some(raw) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sequence = raw.parse().map_err(|_| "无效的媒体序号")?;
        } else if line.starts_with("#EXT-X-KEY:") {
            let a = attributes(line);
            match a.get("METHOD").map(String::as_str) {
                Some("NONE") => {
                    key = None;
                    explicit_iv = None;
                }
                Some("AES-128") => {
                    if a.get("KEYFORMAT").is_some_and(|v| v != "identity") {
                        return Err("暂不检查此密钥格式".into());
                    }
                    let url = base
                        .join(a.get("URI").ok_or("密钥缺少 URI")?)
                        .map_err(|_| "密钥地址无效")?;
                    key = Some(url.to_string());
                    explicit_iv = a.get("IV").map(|v| parse_iv(v)).transpose()?;
                    resources.push(Resource {
                        line: index,
                        url,
                        kind: Kind::Key,
                        range: None,
                        crypto: None,
                    });
                }
                _ => return Err("暂不检查此加密方式".into()),
            }
        } else if line.starts_with("#EXT-X-MAP:") {
            let a = attributes(line);
            let url = base
                .join(a.get("URI").ok_or("初始化文件缺少 URI")?)
                .map_err(|_| "初始化地址无效")?;
            let range = a
                .get("BYTERANGE")
                .map(|v| byte_range(v, Some(0)))
                .transpose()?;
            if key.is_some() && explicit_iv.is_none() {
                return Err("加密初始化文件缺少 IV".into());
            }
            resources.push(Resource {
                line: index,
                url,
                kind: Kind::Init,
                range,
                crypto: key.as_ref().map(|k| (k.clone(), explicit_iv.unwrap())),
            });
        } else if let Some(raw) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            pending_range = Some(raw.to_string());
        } else if !line.is_empty() && !line.starts_with('#') {
            if !duration_pending {
                return Err("分片缺少 EXTINF".into());
            }
            duration_pending = false;
            let url = base.join(line).map_err(|_| "分片地址无效")?;
            let implicit = last_range
                .as_ref()
                .filter(|(u, _)| u == &url)
                .map(|(_, end)| *end);
            let range = pending_range
                .take()
                .map(|v| byte_range(&v, implicit))
                .transpose()?;
            if key.is_some() && range.is_some() {
                return Err("暂不检查加密字节范围流".into());
            }
            last_range = range.map(|(offset, length)| (url.clone(), offset + length));
            resources.push(Resource {
                line: index,
                url,
                kind: Kind::Segment,
                range,
                crypto: key
                    .as_ref()
                    .map(|k| (k.clone(), explicit_iv.unwrap_or(sequence_iv(sequence)))),
            });
            sequence = sequence.checked_add(1).ok_or("媒体序号溢出")?;
        }
    }
    Ok(resources)
}

fn parse_iv(raw: &str) -> Result<[u8; 16], String> {
    let raw = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    if raw.is_empty() || raw.len() > 32 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("AES IV 无效".into());
    }
    let raw = format!("{raw:0>32}");
    let mut iv = [0; 16];
    for (i, b) in iv.iter_mut().enumerate() {
        *b = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).map_err(|_| "AES IV 无效")?;
    }
    Ok(iv)
}
fn sequence_iv(sequence: u64) -> [u8; 16] {
    let mut iv = [0; 16];
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    iv
}

fn valid_media(bytes: &[u8], init: bool) -> bool {
    // Inspect bytes, not MIME alone: CDNs often return application/octet-stream.
    if bytes.len() >= 188 * 3 && (0..3).all(|i| bytes[i * 188] == 0x47) {
        return !init;
    }
    if bytes.len() >= 8
        && [b"ftyp", b"styp", b"moof", b"sidx", b"moov"]
            .iter()
            .any(|t| &bytes[4..8] == *t)
    {
        return true;
    }
    !init
        && (bytes.starts_with(b"WEBVTT")
            || bytes.starts_with(b"\xef\xbb\xbfWEBVTT")
            || bytes.starts_with(b"ID3")
            || (bytes.len() > 7 && bytes[0] == 0xff && bytes[1] & 0xf6 == 0xf0))
}

fn samples(resources: &[Resource]) -> Vec<Resource> {
    let media: Vec<_> = resources
        .iter()
        .filter(|r| matches!(r.kind, Kind::Segment))
        .collect();
    let positions: BTreeSet<_> = [0, media.len() / 2, media.len().saturating_sub(1)]
        .into_iter()
        .collect();
    resources
        .iter()
        .filter(|r| !matches!(r.kind, Kind::Segment))
        .cloned()
        .chain(
            positions
                .into_iter()
                .filter_map(|i| media.get(i).map(|r| (*r).clone())),
        )
        .collect()
}

async fn probe(
    ctx: &RequestContext,
    resource: &Resource,
    url: &Url,
    keys: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, ProbeError> {
    let is_key = matches!(resource.kind, Kind::Key);
    let range = (!is_key).then(|| {
        let (offset, length) = resource.range.unwrap_or((0, SAMPLE_LIMIT as u64));
        (offset, length.min(SAMPLE_LIMIT as u64))
    });
    let limit = if is_key {
        64
    } else {
        range.unwrap().1 as usize
    };
    let (_, mut bytes) = ctx.get(url, range, limit, is_key).await?;
    if is_key {
        return if bytes.len() == 16 {
            Ok(bytes)
        } else {
            Err(ProbeError::bad_media())
        };
    }
    if let Some((key, iv)) = &resource.crypto {
        let key = keys
            .get(key)
            .ok_or_else(|| ProbeError::invalid("AES 密钥检查未完成"))?;
        let length = bytes.len() / 16 * 16;
        bytes.truncate(length);
        cbc::Decryptor::<aes::Aes128>::new_from_slices(key, iv)
            .map_err(|_| ProbeError::bad_media())?
            .decrypt_padded_mut::<NoPadding>(&mut bytes)
            .map_err(|_| ProbeError::bad_media())?;
    }
    if valid_media(&bytes, matches!(resource.kind, Kind::Init)) {
        Ok(bytes)
    } else {
        Err(ProbeError::bad_media())
    }
}

/// Candidates come exclusively from validated user rules. The returned rule has
/// passed all representative resources in its scope, not just a hostname check.
pub(crate) async fn try_alternative_domain(
    ctx: &RequestContext,
    failed: &Resource,
    representative: &[Resource],
    rules: &[MirrorRule],
    keys: &BTreeMap<String, Vec<u8>>,
) -> Option<MirrorRule> {
    for rule in rules {
        let Some(candidate) = rule.candidate(&failed.url) else {
            continue;
        };
        if !ctx.allows_engine_headers(&candidate) {
            continue;
        }
        let mut healthy = true;
        for r in representative
            .iter()
            .filter(|r| rule.candidate(&r.url).is_some())
        {
            let target = rule.candidate(&r.url).unwrap();
            if !ctx.allows_engine_headers(&target) || probe(ctx, r, &target, keys).await.is_err() {
                healthy = false;
                break;
            }
        }
        if healthy {
            return Some(rule.clone());
        }
    }
    None
}

struct CheckState {
    context: RequestContext,
    rules: Vec<MirrorRule>,
    plan: RecoveryPlan,
    keys: BTreeMap<String, Vec<u8>>,
    messages: Vec<String>,
    directory: Arc<TemporaryManifests>,
    nodes: usize,
}

impl CheckState {
    async fn media(&mut self, lines: &mut [String], base: &Url) -> Result<(), String> {
        let resources = media_resources(lines, base)?;
        if !resources.iter().any(|r| matches!(r.kind, Kind::Segment)) {
            return Err("点播清单没有分片".into());
        }
        if resources.len() > 100_000 {
            return Err("分片数量超过检查上限".into());
        }
        let representative = samples(&resources);
        if representative.len() > 48 {
            return Err("密钥或初始化文件数量超过检查上限".into());
        }
        for resource in &representative {
            let target = self.plan.route(&resource.url);
            let mut result = probe(&self.context, resource, &target, &self.keys).await;
            if result.as_ref().is_err_and(|e| e.recoverable) {
                tokio::time::sleep(Duration::from_millis(250)).await;
                result = probe(&self.context, resource, &target, &self.keys).await;
            }
            if let Err(error) = result {
                if !error.recoverable {
                    return Err(format!(
                        "HLS 启动检查失败：{}（{}）；请检查授权、代理或清单有效期",
                        target.host_str().unwrap_or(""),
                        error.reason
                    ));
                }
                // Key validation precedes encrypted segment validation. For a
                // missing key, establish its trusted replacement first.
                let checks = if matches!(resource.kind, Kind::Key) {
                    vec![resource.clone()]
                } else {
                    representative.clone()
                };
                let rule = try_alternative_domain(&self.context,resource,&checks,&self.rules,&self.keys).await
                    .ok_or_else(|| format!("HLS 启动检查失败：{}（{}），没有验证通过的可信备用地址；可在设置中配置备用映射", target.host_str().unwrap_or(""),error.reason))?;
                let candidate = rule.candidate(&resource.url).unwrap();
                let bytes = probe(&self.context, resource, &candidate, &self.keys)
                    .await
                    .map_err(|_| "备用地址复查失败")?;
                self.messages.push(format!(
                    "HLS 启动检查：{} → {}，可信备用地址已通过分片验证",
                    target.origin().ascii_serialization(),
                    candidate.origin().ascii_serialization()
                ));
                if matches!(resource.kind, Kind::Key) {
                    self.keys.insert(resource.url.to_string(), bytes);
                }
                self.plan.routes.retain(|r| {
                    r.from_origin != rule.from_origin || r.path_prefix != rule.path_prefix
                });
                self.plan.routes.push(rule);
            } else if let Ok(bytes) = result {
                if matches!(resource.kind, Kind::Key) {
                    self.keys.insert(resource.url.to_string(), bytes);
                }
            }
        }
        // Recheck the final route combination, including rotated keys and maps.
        for resource in representative
            .iter()
            .filter(|_| !self.plan.routes.is_empty())
        {
            probe(
                &self.context,
                resource,
                &self.plan.route(&resource.url),
                &self.keys,
            )
            .await
            .map_err(|e| format!("HLS 最终线路验证失败：{}", e.reason))?;
        }
        for resource in resources {
            lines[resource.line] = replace_uri(
                &lines[resource.line],
                self.plan.route(&resource.url).as_str(),
            )?;
        }
        Ok(())
    }

    fn playlist<'a>(
        &'a mut self,
        url: Url,
        cfg: &'a TaskConfig,
        depth: usize,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>,
    > {
        Box::pin(async move {
            if depth > 4 || self.nodes >= 16 {
                return Err("HLS 清单嵌套或数量超过检查上限".into());
            }
            self.nodes += 1;
            let node_id = self.nodes;
            let (base, bytes) = self
                .context
                .get(&url, None, MANIFEST_LIMIT, true)
                .await
                .map_err(|e| {
                    format!(
                        "HLS 清单检查失败：{}（{}）",
                        url.host_str().unwrap_or(""),
                        e.reason
                    )
                })?;
            let text = std::str::from_utf8(&bytes).map_err(|_| "HLS 清单不是 UTF-8")?;
            let mut lines: Vec<String> = text.lines().map(|s| s.trim().to_string()).collect();
            if lines.first().map(String::as_str) != Some("#EXTM3U") {
                return Err("HLS 清单返回了无效内容".into());
            }
            if lines.iter().any(|s| s.starts_with("#EXT-X-DEFINE:")) {
                self.messages
                    .push("HLS 启动检查：包含变量定义，沿用原清单交给 RE 处理".into());
                return Ok(None);
            }
            if lines.iter().any(|s| s.starts_with("#EXT-X-STREAM-INF:")) {
                if matches!(cfg.preset, Preset::Custom(_)) || depth != 0 {
                    return Err("主清单选择无法准确映射".into());
                }
                let refs = select_master(&mut lines, cfg)?;
                for (line, child) in refs {
                    let child_url = base.join(&child).map_err(|_| "子清单地址无效")?;
                    let Some(input) = self.playlist(child_url, cfg, depth + 1).await? else {
                        return Ok(None);
                    };
                    lines[line] = replace_uri(&lines[line], &input)?;
                }
            } else {
                if !lines.iter().any(|s| s == "#EXT-X-ENDLIST") {
                    self.messages
                        .push("HLS 启动检查：直播/未结束清单沿用 RE，暂不切换线路".into());
                    return Ok(None);
                }
                if lines.iter().any(|s| {
                    s.starts_with("#EXT-X-KEY:")
                        && (!matches!(attr(s, "METHOD").as_deref(), Some("AES-128" | "NONE"))
                            || attr(s, "KEYFORMAT").is_some_and(|v| v != "identity"))
                }) || (lines.iter().any(|s| {
                    s.starts_with("#EXT-X-KEY:") && attr(s, "METHOD").as_deref() == Some("AES-128")
                }) && lines.iter().any(|s| {
                    s.starts_with("#EXT-X-BYTERANGE:")
                        || s.starts_with("#EXT-X-MAP:") && attr(s, "BYTERANGE").is_some()
                })) {
                    self.messages.push("HLS 启动检查：此加密方式沿用 RE".into());
                    return Ok(None);
                }
                self.media(&mut lines, &base).await?;
            }
            let path = self.directory.0.join(format!("playlist-{node_id}.m3u8"));
            let content = lines.join("\n") + "\n";
            crate::fsutil::atomic_write(&path, content.as_bytes())?;
            Ok(Some(
                Url::from_file_path(&path)
                    .map_err(|_| "临时清单路径无效")?
                    .to_string(),
            ))
        })
    }
}

fn select_master(lines: &mut [String], cfg: &TaskConfig) -> Result<Vec<(usize, String)>, String> {
    let cap = match cfg.preset {
        Preset::Limit720 => 720,
        Preset::Limit1080 => 1080,
        _ => u64::MAX,
    };
    let mut variants = Vec::new();
    for (i, line) in lines
        .iter()
        .enumerate()
        .filter(|(_, s)| s.starts_with("#EXT-X-STREAM-INF:"))
    {
        let a = attributes(line);
        let height = a
            .get("RESOLUTION")
            .and_then(|s| s.split_once('x'))
            .and_then(|(_, h)| h.parse::<u64>().ok());
        if height.is_some_and(|h| h > cap) {
            continue;
        }
        let next = (i + 1..lines.len())
            .find(|n| !lines[*n].is_empty() && !lines[*n].starts_with('#'))
            .ok_or("主清单缺少流地址")?;
        let bandwidth = a
            .get("BANDWIDTH")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        variants.push((bandwidth, i, next, a));
    }
    let (_, chosen, uri, attrs) = variants
        .into_iter()
        .max_by_key(|v| v.0)
        .ok_or("没有符合预设清晰度的 HLS 流")?;
    let mut refs = vec![(uri, lines[uri].clone())];
    let mut keep = BTreeSet::from([chosen, uri]);
    for kind in ["AUDIO", "VIDEO", "SUBTITLES"] {
        let Some(group) = attrs.get(kind) else {
            continue;
        };
        let entries: Vec<_> = lines
            .iter()
            .enumerate()
            .filter(|(_, s)| s.starts_with("#EXT-X-MEDIA:"))
            .filter_map(|(i, s)| {
                let a = attributes(s);
                (a.get("TYPE").is_some_and(|t| t == kind) && a.get("GROUP-ID") == Some(group))
                    .then_some((i, a))
            })
            .collect();
        if let Some((i, a)) = entries.iter().max_by_key(|(_, a)| {
            (
                a.get("DEFAULT").is_some_and(|s| s == "YES"),
                a.get("CHANNELS")
                    .and_then(|s| s.split('/').next())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0),
            )
        }) {
            keep.insert(*i);
            if let Some(uri) = a.get("URI") {
                refs.push((*i, uri.clone()));
            }
        }
    }
    let mut variant_uri = false;
    for (i, line) in lines.iter_mut().enumerate() {
        let stream_tag = line.starts_with("#EXT-X-STREAM-INF:");
        let stream_uri = !line.is_empty() && !line.starts_with('#') && variant_uri;
        if stream_uri {
            variant_uri = false;
        }
        if stream_tag {
            variant_uri = true;
        }
        if (stream_tag
            || stream_uri
            || line.starts_with("#EXT-X-MEDIA:")
            || line.starts_with("#EXT-X-I-FRAME-STREAM-INF:"))
            && !keep.contains(&i)
        {
            line.clear();
        }
        // A frozen local master cannot carry remote steering rules for its file URIs.
        if line.starts_with("#EXT-X-CONTENT-STEERING:") {
            line.clear();
        }
    }
    Ok(refs)
}

pub async fn prepare(
    cfg: &TaskConfig,
    source: &HlsSource,
    rules: &[MirrorRule],
    directory: &Path,
    cookie_text: Option<String>,
) -> Result<PreparedInput, String> {
    validate_rules(rules)?;
    std::fs::create_dir_all(directory).map_err(|_| "无法创建 HLS 临时清单目录")?;
    let guard = Arc::new(TemporaryManifests(directory.to_path_buf()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法设置临时清单权限")?;
    }
    let mut state = CheckState {
        context: RequestContext::new(cfg, source, cookie_text)?,
        rules: rules.to_vec(),
        plan: RecoveryPlan::default(),
        keys: BTreeMap::new(),
        messages: Vec::new(),
        directory: guard.clone(),
        nodes: 0,
    };
    let url = http_url(&source.url)?;
    let checked = tokio::time::timeout(Duration::from_secs(90), state.playlist(url, cfg, 0))
        .await
        .map_err(|_| "HLS 启动检查超时（90s）")??;
    if let Some(input) = checked {
        state
            .messages
            .push("HLS 点播启动检查通过（首、中、尾分片及密钥/初始化文件）".into());
        Ok(PreparedInput {
            input,
            plan: state.plan,
            messages: state.messages,
            _files: Some(guard),
        })
    } else {
        Ok(PreparedInput {
            input: source.url.clone(),
            plan: RecoveryPlan::default(),
            messages: state.messages,
            _files: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };

    type Reply = (u16, Vec<(String, String)>, Vec<u8>);
    struct Server {
        url: String,
        requests: Arc<Mutex<Vec<String>>>,
        done: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.done.store(true, Ordering::SeqCst);
            self.thread.take().unwrap().join().unwrap();
        }
    }
    fn server(handler: impl Fn(&str) -> Reply + Send + 'static) -> Server {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let (log, stop) = (requests.clone(), done.clone());
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut data = Vec::new();
                let mut chunk = [0; 1024];
                while let Ok(n) = stream.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&chunk[..n]);
                    if data.ends_with(b"\r\n\r\n") || data.len() > 16 * 1024 {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&data).to_string();
                if request.is_empty() {
                    continue;
                }
                log.lock().unwrap().push(request.clone());
                let (status, headers, body) = handler(&request);
                let mut head = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        Server {
            url,
            requests,
            done,
            thread: Some(thread),
        }
    }
    fn ts() -> Vec<u8> {
        let mut data = vec![0; 188 * 4];
        for i in 0..4 {
            data[i * 188] = 0x47;
        }
        data
    }
    fn vod(host: &str) -> String {
        format!("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:9\n#EXTINF:1,\n{host}/video/s0.ts?token=a%2Fb&z=2\n#EXT-X-DISCONTINUITY\n#EXTINF:1,\n{host}/video/s1.ts\n#EXTINF:1,\n{host}/video/s2.ts\n#EXT-X-ENDLIST\n")
    }
    fn cfg(url: &str) -> TaskConfig {
        TaskConfig {
            url: url.into(),
            ..Default::default()
        }
    }
    fn temporary() -> PathBuf {
        std::env::temp_dir().join(format!(
            "hls-preflight-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    fn read(prepared: &PreparedInput) -> String {
        std::fs::read_to_string(Url::parse(&prepared.input).unwrap().to_file_path().unwrap())
            .unwrap()
    }
    fn rule(from: &str, to: &str) -> MirrorRule {
        MirrorRule {
            from_origin: from.into(),
            to_origin: to.into(),
            path_prefix: "/video/".into(),
        }
    }

    #[test]
    fn origin_mapping_is_scoped_and_preserves_signed_octets() {
        let mapping = rule("https://cdn.test:8443", "https://mirror.test");
        validate_rules(std::slice::from_ref(&mapping)).unwrap();
        let u =
            Url::parse("https://cdn.test:8443/video/%E4%B8%AD.ts?sig=a%2Fb%3D&x=1&x=2").unwrap();
        assert_eq!(
            mapping.candidate(&u).unwrap().as_str(),
            "https://mirror.test/video/%E4%B8%AD.ts?sig=a%2Fb%3D&x=1&x=2"
        );
        for raw in [
            "https://cdn.test/video/a.ts",
            "https://cdn.test:8443/videos/a.ts",
            "https://other.test:8443/video/a.ts",
        ] {
            assert!(mapping.candidate(&Url::parse(raw).unwrap()).is_none());
        }
        for r in [
            rule("https://a.test", "http://b.test"),
            rule("https://u:secret@a.test", "https://b.test"),
            rule("https://a.test/path", "https://b.test"),
            rule("https://a.test", "https://a.test"),
        ] {
            assert!(validate_rules(&[r]).is_err());
        }
    }

    #[tokio::test]
    async fn switches_only_after_all_backup_samples_pass_and_cleans_up() {
        let backup = server(|_| (200, vec![], ts()));
        let bad = server(|_| (503, vec![], vec![]));
        let playlist = vod(&bad.url);
        let source_server = server(move |_| (200, vec![], playlist.clone().into_bytes()));
        let url = format!("{}/index.m3u8", source_server.url);
        let mut source = crate::m3u8::direct_source(&url);
        source
            .headers
            .insert("Sec-Fetch-Mode".into(), "navigate".into());
        let dir = temporary();
        let ready = prepare(
            &cfg(&url),
            &source,
            &[rule(&bad.url, &backup.url)],
            &dir,
            None,
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "{error}; backup requests: {:?}",
                backup.requests.lock().unwrap()
            )
        });
        let content = read(&ready);
        assert!(content.contains(&format!("{}/video/s0.ts?token=a%2Fb&z=2", backup.url)));
        assert!(content.contains("#EXT-X-DISCONTINUITY"));
        assert!(!content.contains(&bad.url));
        assert_eq!(ready.plan.routes.len(), 1);
        assert_eq!(source.url, url);
        for name in ["s0.ts", "s1.ts", "s2.ts"] {
            assert!(backup
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.contains(name)));
        }
        drop(ready);
        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn rejects_html_and_a_backup_missing_the_last_segment() {
        for broken_tail in [false, true] {
            let backup = server(move |request| {
                if broken_tail && !request.contains("s2.ts") {
                    (200, vec![], ts())
                } else {
                    (200, vec![], b"<html>login required</html>".to_vec())
                }
            });
            let bad = server(|_| (503, vec![], vec![]));
            let playlist = vod(&bad.url);
            let source_server = server(move |_| (200, vec![], playlist.clone().into_bytes()));
            let url = format!("{}/index.m3u8", source_server.url);
            let dir = temporary();
            let err = prepare(
                &cfg(&url),
                &crate::m3u8::direct_source(&url),
                &[rule(&bad.url, &backup.url)],
                &dir,
                None,
            )
            .await
            .unwrap_err();
            assert!(err.contains("没有验证通过"));
            assert!(!err.contains("login required"));
            assert!(!dir.exists());
        }
    }

    #[tokio::test]
    async fn no_rules_means_no_domain_guessing_and_auth_errors_do_not_switch() {
        for status in [403, 503] {
            let backup = server(|_| (200, vec![], ts()));
            let bad = server(move |_| (status, vec![], b"sensitive response".to_vec()));
            let playlist = vod(&bad.url);
            let source_server = server(move |_| (200, vec![], playlist.clone().into_bytes()));
            let url = format!("{}/index.m3u8", source_server.url);
            let rules = if status == 403 {
                vec![rule(&bad.url, &backup.url)]
            } else {
                vec![]
            };
            let err = prepare(
                &cfg(&url),
                &crate::m3u8::direct_source(&url),
                &rules,
                &temporary(),
                None,
            )
            .await
            .unwrap_err();
            assert!(err.contains(&format!("HTTP {status}")));
            assert!(!err.contains("sensitive response"));
            assert!(backup.requests.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn live_and_variable_playlists_stay_remote_without_segment_checks() {
        for text in ["#EXTM3U\n#EXTINF:1,\na.ts\n", "#EXTM3U\n#EXT-X-DEFINE:NAME=\"path\",VALUE=\"a\"\n#EXTINF:1,\n{$path}.ts\n#EXT-X-ENDLIST\n"] {
            let s = server(move |_| (200,vec![],text.as_bytes().to_vec()));
            let url = format!("{}/index.m3u8",s.url);
            let dir = temporary();
            let ready = prepare(&cfg(&url),&crate::m3u8::direct_source(&url),&[],&dir,None).await.unwrap();
            assert_eq!(ready.input,url); assert_eq!(s.requests.lock().unwrap().len(),1); assert!(!dir.exists());
        }
    }

    #[tokio::test]
    async fn uses_task_proxy_and_headers_for_real_resources() {
        let playlist = vod("http://unresolvable.invalid");
        let proxy = server(move |r| {
            if !r
                .to_ascii_lowercase()
                .contains("referer: https://page.test/watch")
                || !r.contains("AgentTest")
            {
                return (403, vec![], vec![]);
            }
            if r.contains("index.m3u8") {
                (200, vec![], playlist.clone().into_bytes())
            } else {
                (200, vec![], ts())
            }
        });
        let url = "http://unresolvable.invalid/index.m3u8";
        let mut source = crate::m3u8::direct_source(url);
        source
            .headers
            .insert("User-Agent".into(), "AgentTest".into());
        source
            .headers
            .insert("Referer".into(), "https://page.test/watch".into());
        let c = TaskConfig {
            proxy: Some(proxy.url.clone()),
            ..cfg(url)
        };
        let ready = prepare(&c, &source, &[], &temporary(), None).await.unwrap();
        assert!(read(&ready).contains("unresolvable.invalid/video/s0.ts"));
        assert!(proxy
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.starts_with("GET http://unresolvable.invalid/video/")));
    }

    #[test]
    fn byte_ranges_and_sequence_iv_are_preserved() {
        let base = Url::parse("https://cdn.test/video/index.m3u8").unwrap();
        let text = "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"100@10\"\n#EXTINF:1,\n#EXT-X-BYTERANGE:752@0\na.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:1,\n#EXT-X-BYTERANGE:752\na.ts\n#EXT-X-ENDLIST\n";
        let lines = text.lines().map(String::from).collect::<Vec<_>>();
        let r = media_resources(&lines, &base).unwrap();
        assert_eq!(r[0].range, Some((10, 100)));
        assert_eq!(r[1].range, Some((0, 752)));
        assert_eq!(r[2].range, Some((752, 752)));
        assert_eq!(sequence_iv(9), parse_iv("0x9").unwrap());
        assert!(byte_range("0@0", None).is_err());
        assert!(byte_range("1", None).is_err());
        assert!(!valid_media(b"<html>not media</html>", false));
    }

    #[tokio::test]
    async fn preserves_explicit_audio_selection_and_master_quality_cap() {
        let master = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"English\",DEFAULT=YES,URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=100,RESOLUTION=1280x720,AUDIO=\"audio\"\nvideo.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=200,RESOLUTION=1920x1080,AUDIO=\"audio\"\nlarge.m3u8\n";
        let s = server(move |r| {
            if r.contains("master.m3u8") {
                (200, vec![], master.as_bytes().to_vec())
            } else if r.contains(".m3u8") {
                (
                    200,
                    vec![],
                    b"#EXTM3U\n#EXTINF:1,\na.ts\n#EXT-X-ENDLIST\n".to_vec(),
                )
            } else {
                (200, vec![], ts())
            }
        });
        let url = format!("{}/master.m3u8", s.url);
        let c = TaskConfig {
            preset: Preset::Limit720,
            ..cfg(&url)
        };
        let ready = prepare(
            &c,
            &crate::m3u8::direct_source(&url),
            &[],
            &temporary(),
            None,
        )
        .await
        .unwrap();
        let root = read(&ready);
        assert!(root.contains("1280x720"));
        assert!(!root.contains("1920x1080"));
        let children: Vec<_> = root.lines().filter(|s| s.starts_with("file:")).collect();
        assert_eq!(children.len(), 1);
        assert_ne!(children[0], ready.input);
        assert!(
            std::fs::read_to_string(Url::parse(children[0]).unwrap().to_file_path().unwrap())
                .unwrap()
                .contains(&format!("{}/a.ts", s.url))
        );
        assert!(!s
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("large.m3u8")));
        let audio = format!("{}/audio.m3u8", s.url);
        let mut source = crate::m3u8::direct_source(&audio);
        source.resolved = true;
        let ready = prepare(
            &TaskConfig {
                preset: Preset::Custom("audio-id".into()),
                ..cfg(&audio)
            },
            &source,
            &[],
            &temporary(),
            None,
        )
        .await
        .unwrap();
        assert!(!read(&ready).contains("STREAM-INF"));
    }

    #[tokio::test]
    async fn raw_credentials_are_not_forwarded_to_an_alternative_origin() {
        let backup = server(|_| (200, vec![], ts()));
        let bad = server(|_| (503, vec![], vec![]));
        let playlist = vod(&bad.url);
        let s = server(move |_| (200, vec![], playlist.clone().into_bytes()));
        let url = format!("{}/index.m3u8", s.url);
        let mut source = crate::m3u8::direct_source(&url);
        source
            .headers
            .insert("Authorization".into(), "Bearer secret".into());
        source.headers.insert("Cookie".into(), "sid=secret".into());
        let err = prepare(
            &cfg(&url),
            &source,
            &[rule(&bad.url, &backup.url)],
            &temporary(),
            None,
        )
        .await
        .unwrap_err();
        assert!(!err.contains("secret"));
        assert!(backup.requests.lock().unwrap().is_empty());
        assert!(!bad
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("secret")));
    }

    #[tokio::test]
    async fn dropping_a_pending_check_removes_its_private_directory() {
        let s = server(|_| {
            std::thread::sleep(Duration::from_millis(400));
            (200, vec![], vec![])
        });
        let url = format!("{}/index.m3u8", s.url);
        let dir = temporary();
        let source = crate::m3u8::direct_source(&url);
        let cfg = cfg(&url);
        let result = tokio::time::timeout(
            Duration::from_millis(60),
            prepare(&cfg, &source, &[], &dir, None),
        )
        .await;
        assert!(result.is_err());
        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn aes_checks_decrypted_media_and_rejects_the_wrong_key() {
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut};
        for wrong in [false, true] {
            let key = [7u8; 16];
            let iv = sequence_iv(9);
            let payload = ts();
            let length = payload.len();
            let mut buffer = payload;
            buffer.resize(length + 16, 0);
            let encrypted = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into())
                .encrypt_padded_mut::<Pkcs7>(&mut buffer, length)
                .unwrap()
                .to_vec();
            let s = server(move |request| {
                if request.contains("index.m3u8") {
                    (200,vec![],b"#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:9\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n#EXTINF:1,\na.ts\n#EXT-X-ENDLIST\n".to_vec())
                } else if request.starts_with("GET /key ") {
                    (200, vec![], vec![if wrong { 8 } else { 7 }; 16])
                } else {
                    (200, vec![], encrypted.clone())
                }
            });
            let url = format!("{}/index.m3u8", s.url);
            let dir = temporary();
            let result = prepare(
                &cfg(&url),
                &crate::m3u8::direct_source(&url),
                &[],
                &dir,
                None,
            )
            .await;
            if wrong {
                assert!(result.is_err());
                assert!(!dir.exists());
            } else {
                let ready = result.unwrap();
                assert!(read(&ready).contains("MEDIA-SEQUENCE:9"));
                assert!(read(&ready).contains("METHOD=AES-128"));
            }
        }
    }

    #[tokio::test]
    async fn byte_range_http_checks_use_the_declared_offsets_and_keep_map() {
        let s = server(|request| {
            if request.contains("index.m3u8") {
                return (200,vec![],b"#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"100@10\"\n#EXTINF:1,\n#EXT-X-BYTERANGE:752@0\na.ts\n#EXTINF:1,\n#EXT-X-BYTERANGE:752\na.ts\n#EXT-X-ENDLIST\n".to_vec());
            }
            let start = if request.contains("init.mp4") {
                10
            } else if request.contains("bytes=752-") {
                752
            } else {
                0
            };
            let body = if start == 10 {
                let mut init = b"\0\0\0\x10ftypiso6\0\0\0\0".to_vec();
                init.resize(100, 0);
                init
            } else {
                ts()
            };
            (
                206,
                vec![(
                    "Content-Range".into(),
                    format!(
                        "bytes {start}-{}/{}",
                        start + body.len() - 1,
                        if start == 10 { 110 } else { 1504 }
                    ),
                )],
                body,
            )
        });
        let url = format!("{}/index.m3u8", s.url);
        let ready = prepare(
            &cfg(&url),
            &crate::m3u8::direct_source(&url),
            &[],
            &temporary(),
            None,
        )
        .await
        .unwrap();
        let text = read(&ready);
        assert!(text.contains("BYTERANGE=\"100@10\""));
        assert!(text.contains("#EXT-X-BYTERANGE:752\n"));
        assert!(s
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("range: bytes=752-1503")));
    }

    #[tokio::test]
    async fn invalid_content_range_is_rejected() {
        let s = server(|request| {
            if request.contains("index.m3u8") {
                (
                    200,
                    vec![],
                    b"#EXTM3U\n#EXTINF:1,\na.ts\n#EXT-X-ENDLIST\n".to_vec(),
                )
            } else {
                (
                    206,
                    vec![("Content-Range".into(), "bytes 1-752/1000".into())],
                    ts(),
                )
            }
        });
        let url = format!("{}/index.m3u8", s.url);
        assert!(prepare(
            &cfg(&url),
            &crate::m3u8::direct_source(&url),
            &[],
            &temporary(),
            None
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn redirects_resolve_relative_resources_from_the_final_playlist_url() {
        let s = server(|request| {
            if request.starts_with("GET /index.m3u8 ") {
                (
                    302,
                    vec![("Location".into(), "/asset/list.m3u8".into())],
                    vec![],
                )
            } else if request.contains("list.m3u8") {
                (
                    200,
                    vec![],
                    b"#EXTM3U\n#EXTINF:1,\na.ts\n#EXT-X-ENDLIST\n".to_vec(),
                )
            } else {
                (200, vec![], ts())
            }
        });
        let url = format!("{}/index.m3u8", s.url);
        let ready = prepare(
            &cfg(&url),
            &crate::m3u8::direct_source(&url),
            &[],
            &temporary(),
            None,
        )
        .await
        .unwrap();
        assert!(read(&ready).contains(&format!("{}/asset/a.ts", s.url)));
    }

    #[tokio::test]
    #[ignore = "optional remote startup-only regression via TEST_REMOTE_HLS_URL and mirror origins"]
    async fn real_remote_hls_preflight() {
        let url = std::env::var("TEST_REMOTE_HLS_URL").unwrap();
        let from = std::env::var("TEST_REMOTE_HLS_FROM").unwrap();
        let to = std::env::var("TEST_REMOTE_HLS_TO").unwrap();
        let mut source = crate::m3u8::direct_source(&url);
        source
            .headers
            .insert("User-Agent".into(), "Mozilla/5.0".into());
        source.headers.insert("Referer".into(), url.clone());
        let r = MirrorRule {
            from_origin: from,
            to_origin: to,
            path_prefix: "/videos/".into(),
        };
        let ready = prepare(&cfg(&url), &source, &[r], &temporary(), None)
            .await
            .unwrap();
        assert_eq!(ready.plan.routes.len(), 1);
        assert!(read(&ready).contains(&ready.plan.routes[0].to_origin));
        println!(
            "Remote startup check and trusted fallback succeeded; media download was not started."
        );
    }
}
