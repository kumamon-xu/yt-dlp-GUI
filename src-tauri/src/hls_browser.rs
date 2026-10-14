//! Generic player observation through an isolated, installed Chromium browser.
//! No site rules, user profile, certificate bypass, playback, or media download.
use crate::command::{Preset, TaskConfig};
use crate::hls_preflight::ProbeError;
use crate::hls_preflight::{http_url, Progress};
use crate::m3u8::HlsSource;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

fn find_browser(explicit: Option<&str>) -> Result<PathBuf, String> {
    if let Some(path) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Path::new(path)
            .is_file()
            .then(|| PathBuf::from(path))
            .ok_or_else(|| "浏览器程序路径无效".into());
    }
    let mut paths = Vec::new();
    #[cfg(windows)]
    for base in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(base) {
            for relative in [
                "Microsoft/Edge/Application/msedge.exe",
                "Google/Chrome/Application/chrome.exe",
            ] {
                paths.push(PathBuf::from(&base).join(relative));
            }
        }
    }
    #[cfg(target_os = "macos")]
    for path in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ] {
        paths.push(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in [
                "chromium",
                "chromium-browser",
                "google-chrome",
                "microsoft-edge",
                "chrome.exe",
                "msedge.exe",
            ] {
                paths.push(dir.join(name));
            }
        }
    }
    paths.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        "动态 HLS 解析需要 Edge、Chrome 或 Chromium；请在 HLS 设置中指定浏览器程序".into()
    })
}

struct Browser {
    child: tokio::process::Child,
    profile: PathBuf,
}
impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(pid) = self.child.id() {
            crate::kill_process_tree(pid);
        }
        let _ = self.child.start_kill();
        // Only this newly created, private profile is removed.
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
struct Cdp {
    socket: Socket,
    next: u64,
    pending: Vec<Value>,
    fetch_url: Option<String>,
}
impl Cdp {
    async fn send(&mut self, method: &str, params: Value) -> Result<u64, String> {
        self.send_to(None, method, params).await
    }
    async fn send_to(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<u64, String> {
        self.next += 1;
        let mut message = json!({"id":self.next,"method":method,"params":params});
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::Text(message.to_string().into()))
            .await
            .map_err(|_| "浏览器调试连接已关闭")?;
        Ok(self.next)
    }
    async fn read(&mut self) -> Result<Value, String> {
        loop {
            match self.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return serde_json::from_str(&text).map_err(|_| "浏览器响应格式无效".into())
                }
                Some(Ok(Message::Ping(data))) => {
                    self.socket
                        .send(Message::Pong(data))
                        .await
                        .map_err(|_| "浏览器连接失败")?;
                }
                Some(Ok(_)) => {}
                _ => return Err("浏览器调试连接已关闭".into()),
            }
        }
    }
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.call_to(None, method, params).await
    }
    async fn call_to(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let id = self.send_to(session, method, params).await?;
        loop {
            let msg = self.read().await?;
            if msg["id"] == id {
                if msg.get("error").is_some() {
                    return Err(format!("浏览器操作失败：{method}"));
                }
                return Ok(msg["result"].clone());
            }
            if msg.get("method").is_some() {
                if self.fetch_url.is_some() && msg["method"] == "Fetch.requestPaused" {
                    self.intercept(&msg).await?;
                    continue;
                }
                if self.pending.len() >= 4096 {
                    return Err("页面请求数量超过解析上限".into());
                }
                self.pending.push(msg);
            }
        }
    }
    async fn intercept(&mut self, msg: &Value) -> Result<(), String> {
        let p = &msg["params"];
        let r = &p["request"];
        let allow = self.fetch_url.as_deref() == r["url"].as_str()
            && (r["method"] == "GET" || r["method"] == "OPTIONS");
        self.send_to(
            msg["sessionId"].as_str(),
            if allow {
                "Fetch.continueRequest"
            } else {
                "Fetch.failRequest"
            },
            if allow {
                json!({"requestId":p["requestId"]})
            } else {
                json!({"requestId":p["requestId"],"errorReason":"BlockedByClient"})
            },
        )
        .await?;
        Ok(())
    }
}

/// Kept alive by a prepared task. Serialized CDP fetches use Chromium's TLS/network
/// stack and the isolated page's cookie/referrer/CORS context, with bounded bodies.
pub struct Transport {
    cdp: tokio::sync::Mutex<Cdp>,
    _browser: Browser,
    _permit: tokio::sync::OwnedSemaphorePermit,
    context_id: Option<u64>,
    session_id: Option<String>,
}
impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BrowserTransport")
    }
}
impl Transport {
    pub async fn get(
        &self,
        url: &Url,
        headers: &BTreeMap<String, String>,
        range: Option<(u64, u64)>,
        limit: usize,
        complete: bool,
    ) -> Result<(Url, Vec<u8>), ProbeError> {
        let operation = async {
            let mut cdp = self.cdp.lock().await;
            cdp.fetch_url = Some(url.to_string());
            while let Some(event) = cdp.pending.pop() {
                if event["method"] == "Fetch.requestPaused" {
                    cdp.intercept(&event).await.map_err(|_| browser_error())?;
                }
            }
            let credentials = if headers.keys().any(|k| k.eq_ignore_ascii_case("cookie")) {
                "include"
            } else {
                "same-origin"
            };
            let mut headers: BTreeMap<_, _> = headers
                .iter()
                .filter(|(k, _)| {
                    ![
                        "user-agent",
                        "referer",
                        "origin",
                        "cookie",
                        "host",
                        "accept-encoding",
                    ]
                    .contains(&k.to_ascii_lowercase().as_str())
                        && !k.to_ascii_lowercase().starts_with("sec-")
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if let Some((offset, length)) = range {
                headers.insert(
                    "Range".into(),
                    format!(
                        "bytes={offset}-{}",
                        offset.saturating_add(length).saturating_sub(1)
                    ),
                );
            }
            let options = json!({"url":url.as_str(),"headers":headers,"credentials":credentials,"limit":limit,"complete":complete});
            // JSON is passed as a JS literal, never shell text; URLs/auth stay private.
            let script = format!(
                r#"(async () => {{
              const o = {options};
              try {{
                globalThis.__ytdlpAbort?.abort(); globalThis.__ytdlpAbort = new AbortController();
                const r = await fetch(o.url,{{headers:o.headers,credentials:o.credentials,redirect:'error',cache:'no-store',signal:globalThis.__ytdlpAbort.signal}});
                const reader = r.body?.getReader(), chunks = []; let size = 0;
                if (reader) {{ while (true) {{ const item = await reader.read(); if(item.done)break;
                  const remaining = o.limit + (o.complete ? 1 : 0) - size;
                  chunks.push(item.value.slice(0,remaining)); size += Math.min(item.value.length,remaining);
                  if(size >= o.limit + (o.complete ? 1 : 0)){{await reader.cancel();break;}}
                }} }}
                if(size > o.limit)return {{error:'limit'}};
                const bytes = new Uint8Array(size); let offset=0;
                for(const chunk of chunks){{bytes.set(chunk,offset);offset+=chunk.length;}}
                let binary='';for(let i=0;i<size;i+=8192)binary+=String.fromCharCode(...bytes.subarray(i,i+8192));
                return {{status:r.status,url:r.url,range:r.headers.get('content-range'),body:btoa(binary)}};
              }} catch (_) {{ return {{error:'network'}}; }}
            }})()"#
            );
            let mut params = json!({"expression":script,"awaitPromise":true,"returnByValue":true});
            if let Some(id) = self.context_id {
                params["contextId"] = json!(id);
            }
            let value = cdp
                .call_to(self.session_id.as_deref(), "Runtime.evaluate", params)
                .await
                .map_err(|_| browser_error())?;
            // CDP exposes response headers even when CORS does not expose
            // Content-Range to page JavaScript. Validate against the wire header.
            let wire_range = cdp.pending.iter().rev().find_map(|event| {
                (event["method"] == "Network.responseReceived"
                    && event["params"]["response"]["url"].as_str() == Some(url.as_str()))
                .then(|| {
                    event["params"]["response"]["headers"]
                        .as_object()?
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("content-range"))?
                        .1
                        .as_str()
                        .map(String::from)
                })
                .flatten()
            });
            cdp.pending.clear();
            let result = &value["result"]["value"];
            if result["error"] == "limit" {
                return Err(ProbeError {
                    reason: "浏览器响应超过检查大小上限".into(),
                    recoverable: false,
                });
            }
            let status = result["status"].as_u64().ok_or_else(browser_error)?;
            if status != 200 && status != 206 {
                return Err(ProbeError {
                    reason: format!("HTTP {status}"),
                    recoverable: matches!(status, 404 | 410 | 500..=599),
                });
            }
            if status == 206 {
                let (offset, length) = range.ok_or_else(browser_error)?;
                let (span, total) = result["range"]
                    .as_str()
                    .or(wire_range.as_deref())
                    .and_then(|s| s.strip_prefix("bytes "))
                    .and_then(|s| s.split_once('/'))
                    .ok_or_else(browser_error)?;
                let (start, end) = span.split_once('-').ok_or_else(browser_error)?;
                let (start, end, total) = (
                    start.parse::<u64>().map_err(|_| browser_error())?,
                    end.parse::<u64>().map_err(|_| browser_error())?,
                    total.parse::<u64>().map_err(|_| browser_error())?,
                );
                if total == 0
                    || start != offset
                    || end
                        != offset
                            .saturating_add(length)
                            .saturating_sub(1)
                            .min(total - 1)
                {
                    return Err(browser_error());
                }
            } else if range.is_some_and(|r| r.0 != 0) {
                return Err(browser_error());
            }
            let data = base64::engine::general_purpose::STANDARD
                .decode(result["body"].as_str().ok_or_else(browser_error)?)
                .map_err(|_| browser_error())?;
            if data.is_empty() {
                return Err(browser_error());
            }
            Ok((
                http_url(result["url"].as_str().ok_or_else(browser_error)?)
                    .map_err(|_| browser_error())?,
                data,
            ))
        };
        tokio::time::timeout(Duration::from_secs(25), operation)
            .await
            .map_err(|_| ProbeError {
                reason: "浏览器资源请求超时".into(),
                recoverable: true,
            })?
    }
}
fn browser_error() -> ProbeError {
    ProbeError {
        reason: "浏览器资源请求失败（TLS/网络或跨域策略）".into(),
        recoverable: true,
    }
}

const OBSERVE_SCRIPT: &str = r#"
window.open = () => null;
navigator.sendBeacon = () => false;
HTMLMediaElement.prototype.play = () => Promise.resolve();
HTMLFormElement.prototype.submit = () => {};
"#;
const SOURCES_SCRIPT: &str = r#"JSON.stringify({
  title: document.title.slice(0,240),
  hlsSources: Array.from(document.querySelectorAll('video source')).filter(s => /mpegurl/i.test(s.type)).map(s => s.src),
  sources: Array.from(document.querySelectorAll('video')).slice(0,2).map(v =>
    [v.currentSrc, v.src, ...Array.from(v.querySelectorAll('source')).map(s => s.src)]
      .filter(s => /^https?:/i.test(s)))
})"#;

fn media_request(url: &Url) -> bool {
    [
        ".ts", ".m4s", ".key", ".mp4", ".mp3", ".aac", ".webm", ".flv",
    ]
    .iter()
    .any(|ext| url.path().to_ascii_lowercase().ends_with(ext))
}
fn interception_patterns() -> Value {
    json!({"patterns":[{"urlPattern":"*","requestStage":"Request"},{"urlPattern":"*","requestStage":"Response"}]})
}
fn response_header<'a>(p: &'a Value, name: &str) -> Option<&'a str> {
    p["responseHeaders"].as_array()?.iter().find(|h| {
        h["name"]
            .as_str()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })?["value"]
        .as_str()
}
fn combined_dom(doms: &BTreeMap<Option<String>, Value>) -> Value {
    let root = doms.get(&None).cloned().unwrap_or_default();
    if root["sources"].as_array().is_some_and(|groups| {
        groups.iter().any(|g| {
            g.as_array().is_some_and(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|s| http_url(s).is_ok())
            })
        })
    }) {
        return root;
    }
    let sources: Vec<_> = doms
        .values()
        .flat_map(|dom| dom["sources"].as_array().into_iter().flatten().cloned())
        .collect();
    let hls_sources: Vec<_> = doms
        .values()
        .flat_map(|dom| dom["hlsSources"].as_array().into_iter().flatten().cloned())
        .collect();
    json!({"title":root["title"],"sources":sources,"hlsSources":hls_sources})
}
fn headers(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            let v = v.as_str()?;
            // Browser hop-by-hop/client-hint headers are regenerated by the native client.
            (!k.starts_with(':')
                && !k.to_ascii_lowercase().starts_with("sec-ch-")
                && ![
                    "host",
                    "content-length",
                    "connection",
                    "accept-encoding",
                    "range",
                ]
                .contains(&k.to_ascii_lowercase().as_str())
                && k.len() < 128
                && v.len() <= 8192
                && !v.contains(['\r', '\n']))
            .then(|| (k.to_ascii_lowercase(), v.to_string()))
        })
        .collect()
}

/// Prefer the primary player, never related episodes or arbitrary page URLs.
fn choose_source(
    dom: &Value,
    observed: &[String],
    expected: Option<&HlsSource>,
) -> Result<String, String> {
    let groups = dom["sources"].as_array().cloned().unwrap_or_default();
    let raw: Vec<_> = groups
        .iter()
        .filter_map(Value::as_array)
        .filter(|group| {
            group
                .iter()
                .filter_map(Value::as_str)
                .any(|s| http_url(s).is_ok())
        })
        .collect();
    if raw.len() > 1 {
        return Err("页面有多个视频播放器，无法确认当前视频".into());
    }
    let groups: Vec<Vec<String>> = groups
        .iter()
        .filter_map(Value::as_array)
        .map(|group| {
            group
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| {
                    crate::m3u8::is_m3u8_url(s)
                        || observed.iter().any(|u| u == s)
                        || dom["hlsSources"]
                            .as_array()
                            .is_some_and(|urls| urls.iter().any(|u| u.as_str() == Some(*s)))
                        || expected.is_some_and(|expected| {
                            http_url(s)
                                .ok()
                                .zip(http_url(&expected.url).ok())
                                .is_some_and(|(a, b)| a.path() == b.path())
                        })
                })
                .map(String::from)
                .collect()
        })
        .filter(|g: &Vec<String>| !g.is_empty())
        .collect();
    if groups.len() > 1 {
        return Err("页面有多个视频播放器，无法确认当前视频".into());
    }
    let primary = groups.first().and_then(|g| g.first());
    if !raw.is_empty() && primary.is_none() {
        return Err("当前主播放器未提供 HLS，拒绝改用页面上的其他媒体请求".into());
    }
    let all: Vec<_> = primary.into_iter().chain(observed.iter()).collect();
    if let Some(expected) = expected {
        let old = http_url(&expected.url)?;
        return all
            .into_iter()
            .find(|s| {
                http_url(s).is_ok_and(|u| {
                    crate::hls_preflight::asset_identity(&u)
                        == crate::hls_preflight::asset_identity(&old)
                        && !(old.scheme() == "https" && u.scheme() != "https")
                })
            })
            .cloned()
            .ok_or_else(|| "播放器刷新后的流无法与已选流准确对应".into());
    }
    if let Some(primary) = primary {
        return Ok(primary.clone());
    }
    let unique: std::collections::BTreeSet<_> = observed.iter().collect();
    if unique.len() == 1 {
        return Ok((*unique.into_iter().next().unwrap()).clone());
    }
    Err("动态解析未找到唯一的当前 HLS 流（可能需要登录、手动播放或明确选流）".into())
}

pub async fn resolve(
    cfg: &TaskConfig,
    expected: Option<&HlsSource>,
    path: Option<&str>,
    progress: Progress,
) -> Result<HlsSource, String> {
    if matches!(cfg.preset, Preset::Custom(_)) && expected.is_none() {
        return Err("自定义流选择继续使用 yt-dlp，浏览器解析不能替代具体格式选择".into());
    }
    http_url(&cfg.url)?;
    let browser = find_browser(path)?;
    progress("HLS 自动分析：启动隔离浏览器，观察当前播放器清单和请求头");
    tokio::time::timeout(
        Duration::from_secs(35),
        observe(cfg, expected, &browser, progress),
    )
    .await
    .map_err(|_| "浏览器 HLS 解析超时（35s）".to_string())?
}

async fn observe(
    cfg: &TaskConfig,
    expected: Option<&HlsSource>,
    executable: &Path,
    progress: Progress,
) -> Result<HlsSource, String> {
    static SLOTS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    let permit = SLOTS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "浏览器解析不可用")?;
    let profile = std::env::temp_dir().join(format!(
        "ytdlp-hls-browser-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir(&profile).map_err(|_| "无法创建浏览器临时目录")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法设置浏览器临时目录权限")?;
    }
    let mut cmd = crate::no_window_cmd(executable);
    cmd.args([
        "--headless=new",
        "--remote-debugging-port=0",
        "--remote-debugging-address=127.0.0.1",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-background-networking",
        "--disable-sync",
        "--disable-extensions",
        "--disable-component-update",
        "--disable-default-apps",
        "--mute-audio",
    ])
    .arg(format!("--user-data-dir={}", profile.display()))
    .arg("about:blank")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .kill_on_drop(true);
    if let Some(proxy) = cfg.proxy.as_deref().filter(|s| !s.is_empty()) {
        let proxy = Url::parse(proxy).map_err(|_| "浏览器代理无效")?;
        if !proxy.username().is_empty() || proxy.password().is_some() {
            let _ = std::fs::remove_dir(&profile);
            return Err("带用户名密码的代理暂不支持浏览器解析；原生检查仍可使用该代理".into());
        }
        cmd.arg(format!("--proxy-server={proxy}"));
    } else {
        cmd.arg("--no-proxy-server");
    }
    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = std::fs::remove_dir(&profile);
            return Err("无法启动 HLS 解析浏览器".into());
        }
    };
    let browser = Browser {
        child,
        profile: profile.clone(),
    };
    let local = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| "无法连接本地浏览器")?;
    let port = loop {
        if let Ok(text) = tokio::fs::read_to_string(profile.join("DevToolsActivePort")).await {
            if let Some(port) = text.lines().next().and_then(|s| s.parse::<u16>().ok()) {
                break port;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let tabs: Value = local
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .send()
        .await
        .map_err(|_| "无法读取浏览器标签")?
        .json()
        .await
        .map_err(|_| "浏览器标签格式无效")?;
    let endpoint = tabs
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["type"] == "page")
        .and_then(|v| v["webSocketDebuggerUrl"].as_str())
        .ok_or("浏览器未创建解析标签")?;
    let (socket, _) = tokio_tungstenite::connect_async(endpoint)
        .await
        .map_err(|_| "无法连接浏览器调试协议")?;
    let mut cdp = Cdp {
        socket,
        next: 0,
        pending: Vec::new(),
        fetch_url: None,
    };
    cdp.call("Page.enable", json!({})).await?;
    cdp.call("Runtime.enable", json!({})).await?;
    cdp.call("Target.setAutoAttach",json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true,"filter":[{"type":"iframe","exclude":false},{"exclude":true}]})).await?;
    cdp.call(
        "Network.enable",
        json!({"maxTotalBufferSize":4194304,"maxResourceBufferSize":1048576}),
    )
    .await?;
    cdp.call("Network.setCacheDisabled", json!({"cacheDisabled":true}))
        .await?;
    cdp.call("Network.setBypassServiceWorker", json!({"bypass":true}))
        .await?;
    cdp.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source":OBSERVE_SCRIPT}),
    )
    .await?;
    if let Some(text) = expected.and_then(|s| s.cookie_text.clone()).or(cfg
        .cookies_file
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(std::fs::read_to_string)
        .transpose()
        .map_err(|_| "无法读取解析 Cookies")?)
    {
        let cookies = browser_cookies(&text);
        if !cookies.is_empty() {
            cdp.call("Network.setCookies", json!({"cookies":cookies}))
                .await?;
        }
    }
    cdp.call("Fetch.enable", interception_patterns()).await?;
    cdp.send("Page.navigate", json!({"url":cfg.url})).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(18);
    let mut request_headers = BTreeMap::new();
    let mut header_priority = BTreeMap::new();
    let mut frames = BTreeMap::new();
    let mut contexts = BTreeMap::new();
    let mut observed = Vec::new();
    let mut doms: BTreeMap<Option<String>, Value> = BTreeMap::new();
    let mut sessions: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut poll = tokio::time::interval(Duration::from_millis(600));
    let mut dom_commands = BTreeMap::new();
    let mut requests = 0;
    let mut ready_at = None;
    while tokio::time::Instant::now() < deadline {
        let event = if let Some(event) = cdp.pending.pop() {
            event
        } else {
            tokio::select! {
                event = cdp.read() => event?,
                _ = poll.tick() => {
                    if dom_commands.is_empty() {
                        let id = cdp.send("Runtime.evaluate",json!({"expression":SOURCES_SCRIPT,"returnByValue":true})).await?;
                        dom_commands.insert(id,None);
                        for session in &sessions {
                            let id = cdp.send_to(Some(session),"Runtime.evaluate",json!({"expression":SOURCES_SCRIPT,"returnByValue":true})).await?;
                            dom_commands.insert(id,Some(session.clone()));
                        }
                    }
                    if ready_at.is_some_and(|at| tokio::time::Instant::now() > at) { break; }
                    continue;
                }
                _ = tokio::time::sleep_until(deadline) => break,
            }
        };
        if let Some(session) = event["id"].as_u64().and_then(|id| dom_commands.remove(&id)) {
            if let Some(text) = event["result"]["result"]["value"].as_str() {
                doms.insert(session, serde_json::from_str(text).unwrap_or_default());
                if choose_source(&combined_dom(&doms), &observed, expected)
                    .is_ok_and(|source| header_priority.get(&source) == Some(&2))
                    && ready_at.is_none()
                {
                    ready_at = Some(tokio::time::Instant::now() + Duration::from_secs(3));
                }
            }
        }
        if event["method"] == "Target.attachedToTarget" {
            let p = &event["params"];
            if let Some(session) = p["sessionId"]
                .as_str()
                .filter(|_| p["targetInfo"]["type"] == "iframe")
            {
                if sessions.len() >= 16 {
                    return Err("嵌入播放器数量超过解析上限".into());
                }
                sessions.insert(session.to_string());
                cdp.send_to(Some(session), "Page.enable", json!({})).await?;
                cdp.send_to(Some(session), "Runtime.enable", json!({}))
                    .await?;
                cdp.send_to(
                    Some(session),
                    "Network.enable",
                    json!({"maxTotalBufferSize":4194304,"maxResourceBufferSize":1048576}),
                )
                .await?;
                cdp.send_to(
                    Some(session),
                    "Network.setBypassServiceWorker",
                    json!({"bypass":true}),
                )
                .await?;
                cdp.send_to(
                    Some(session),
                    "Page.addScriptToEvaluateOnNewDocument",
                    json!({"source":OBSERVE_SCRIPT}),
                )
                .await?;
                cdp.send_to(Some(session), "Fetch.enable", interception_patterns())
                    .await?;
                cdp.send_to(Some(session),"Target.setAutoAttach",json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true,"filter":[{"type":"iframe","exclude":false},{"exclude":true}]})).await?;
                cdp.send_to(Some(session), "Runtime.runIfWaitingForDebugger", json!({}))
                    .await?;
            }
        } else if event["method"] == "Target.detachedFromTarget" {
            if let Some(session) = event["params"]["sessionId"].as_str() {
                sessions.remove(session);
                doms.remove(&Some(session.to_string()));
            }
        }
        if event["method"] == "Runtime.executionContextCreated" {
            let context = &event["params"]["context"];
            if context["auxData"]["isDefault"] == true {
                if let (Some(frame), Some(id)) = (
                    context["auxData"]["frameId"].as_str(),
                    context["id"].as_u64(),
                ) {
                    contexts.insert(
                        frame.to_string(),
                        (id, event["sessionId"].as_str().map(String::from)),
                    );
                }
            }
        } else if event["method"] == "Runtime.executionContextDestroyed" {
            let id = event["params"]["executionContextId"].as_u64();
            contexts.retain(|_, value| {
                Some(value.0) != id || value.1.as_deref() != event["sessionId"].as_str()
            });
        }
        if event["method"] != "Fetch.requestPaused" {
            continue;
        }
        requests += 1;
        if requests > 1024 {
            return Err("页面请求数量超过解析上限".into());
        }
        let p = &event["params"];
        let r = &p["request"];
        let url = r["url"].as_str().and_then(|s| http_url(s).ok());
        if p["responseStatusCode"].is_number() {
            let mime = response_header(p, "content-type")
                .unwrap_or("")
                .to_ascii_lowercase();
            let declared = crate::m3u8::is_m3u8_url(r["url"].as_str().unwrap_or(""))
                || mime.contains("mpegurl");
            if declared
                && observed.len() < 32
                && !observed
                    .iter()
                    .any(|s| Some(s.as_str()) == r["url"].as_str())
            {
                if let Some(url) = &url {
                    observed.push(url.to_string());
                }
            }
            let length = response_header(p, "content-length").and_then(|s| s.parse::<u64>().ok());
            let code = ["Document", "Script", "Stylesheet"]
                .iter()
                .any(|s| p["resourceType"] == *s);
            let limit = if code {
                8 * 1024 * 1024
            } else if declared || mime.contains("json") {
                1024 * 1024
            } else {
                64 * 1024
            };
            let blocked = length.is_some_and(|n| n > limit)
                || !declared
                    && !code
                    && (mime.starts_with("video/")
                        || mime.starts_with("audio/")
                        || mime.contains("octet-stream") && length.is_none());
            cdp.send_to(
                event["sessionId"].as_str(),
                if blocked {
                    "Fetch.failRequest"
                } else {
                    "Fetch.continueRequest"
                },
                if blocked {
                    json!({"requestId":p["requestId"],"errorReason":"BlockedByClient"})
                } else {
                    json!({"requestId":p["requestId"]})
                },
            )
            .await?;
            continue;
        }
        if let Some(url) = &url {
            if crate::m3u8::is_m3u8_url(url.as_str())
                && observed.len() < 32
                && !observed.contains(&url.to_string())
            {
                observed.push(url.to_string());
            }
            if r["method"] == "GET"
                && (crate::m3u8::is_m3u8_url(url.as_str())
                    || media_request(url)
                    || p["resourceType"] == "XHR"
                    || p["resourceType"] == "Fetch")
                && request_headers.len() < 128
            {
                // Native <video> preload may retry the same URL without player
                // authentication. Prefer the actual script's Fetch/XHR request.
                let priority = if p["resourceType"] == "XHR" || p["resourceType"] == "Fetch" {
                    2
                } else {
                    1
                };
                if header_priority
                    .get(url.as_str())
                    .is_none_or(|previous| *previous <= priority)
                {
                    header_priority.insert(url.to_string(), priority);
                    if let Some(frame) = p["frameId"].as_str() {
                        frames.insert(url.to_string(), frame.to_string());
                    }
                    request_headers.insert(url.to_string(), headers(&r["headers"]));
                }
            }
        }
        let preflight = r["method"] == "OPTIONS"
            && r["headers"].as_object().is_some_and(|headers| {
                headers.iter().any(|(k, v)| {
                    k.eq_ignore_ascii_case("access-control-request-method") && v == "GET"
                })
            });
        let blocked = !(r["method"] == "GET" || preflight)
            || url.is_none()
            || ["Image", "Media", "Font"]
                .iter()
                .any(|s| p["resourceType"] == *s)
            || url.as_ref().is_some_and(media_request);
        cdp.send_to(
            event["sessionId"].as_str(),
            if blocked {
                "Fetch.failRequest"
            } else {
                "Fetch.continueRequest"
            },
            if blocked {
                json!({"requestId":p["requestId"],"errorReason":"BlockedByClient"})
            } else {
                json!({"requestId":p["requestId"]})
            },
        )
        .await?;
    }
    let dom = combined_dom(&doms);
    let url = choose_source(&dom, &observed, expected)?;
    let mut source = expected
        .cloned()
        .unwrap_or_else(|| crate::m3u8::direct_source(&url));
    source.url = url;
    if expected.is_none() {
        source.title = dom["title"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("HLS video")
            .into();
    }
    // Safe global headers come from the selected manifest only. Scoped headers stay
    // on their exact observed URL, so x-cdn-auth never leaks to key/segment origins.
    if let Some(observed_headers) = request_headers.get(&source.url) {
        source.headers.clear();
        for (k, v) in observed_headers {
            if [
                "user-agent",
                "referer",
                "origin",
                "accept",
                "accept-language",
            ]
            .contains(&k.as_str())
            {
                source.headers.insert(k.clone(), v.clone());
            }
        }
    }
    source.request_headers = request_headers;
    let context = frames
        .get(&source.url)
        .and_then(|frame| contexts.get(frame))
        .cloned();
    let context_id = context.as_ref().map(|v| v.0);
    let session_id = context.and_then(|v| v.1);
    let cookies = cdp
        .call(
            "Network.getCookies",
            json!({"urls":source.request_headers.keys().take(128).collect::<Vec<_>>()}),
        )
        .await?;
    source.cookie_text = Some(export_cookies(&cookies["cookies"]));
    cdp.send("Page.stopLoading", json!({})).await?;
    // Stop the player's timers; subsequent media requests are allowed only for the
    // exact active resource requested by our local task service.
    for (id, session) in contexts.values() {
        cdp.send_to(session.as_deref(),"Runtime.evaluate",json!({"contextId":id,"expression":"document.querySelectorAll('video,audio').forEach(v=>{v.removeAttribute('src');v.remove();});for(let n=1;n<10000;n++){clearTimeout(n);clearInterval(n);}"})).await?;
    }
    source.browser = Some(Arc::new(Transport {
        cdp: tokio::sync::Mutex::new(cdp),
        _browser: browser,
        _permit: permit,
        context_id,
        session_id,
    }));
    progress(&format!(
        "HLS 浏览器解析完成：{}，已保留清单和资源各自的请求头",
        http_url(&source.url)?.origin().ascii_serialization()
    ));
    Ok(source)
}

fn browser_cookies(text: &str) -> Vec<Value> {
    text.lines().filter_map(|line| {
        let http_only = line.starts_with("#HttpOnly_");
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        if line.starts_with('#') { return None; }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 7 { return None; }
        let expiry = fields[4].parse::<u64>().ok()?;
        if expiry != 0 && expiry <= SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() { return None; }
        let mut cookie = json!({"domain":fields[0],"path":fields[2],"secure":fields[3]=="TRUE","httpOnly":http_only,"name":fields[5],"value":fields[6]});
        if expiry != 0 { cookie["expires"] = json!(expiry); }
        Some(cookie)
    }).take(1024).collect()
}

fn export_cookies(value: &Value) -> String {
    let mut text = String::from("# Netscape HTTP Cookie File\n");
    for cookie in value.as_array().into_iter().flatten().take(1024) {
        let fields =
            ["domain", "path", "name", "value"].map(|key| cookie[key].as_str().unwrap_or(""));
        if fields.iter().any(|s| s.contains(['\t', '\r', '\n'])) || fields[0].is_empty() {
            continue;
        }
        let expiry = cookie["expires"]
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.)
            .unwrap_or(0.) as u64;
        text.push_str(&format!(
            "{}{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            if cookie["httpOnly"] == true {
                "#HttpOnly_"
            } else {
                ""
            },
            fields[0],
            if fields[0].starts_with('.') {
                "TRUE"
            } else {
                "FALSE"
            },
            fields[1],
            if cookie["secure"] == true {
                "TRUE"
            } else {
                "FALSE"
            },
            expiry,
            fields[2],
            fields[3]
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primary_player_excludes_related_episode_and_ambiguous_players() {
        let dom = json!({"sources":[["https://cdn.test/asset/current.m3u8"]]});
        let seen = vec![
            "https://ads.test/ad.m3u8".into(),
            "https://cdn.test/asset/other.m3u8".into(),
        ];
        assert_eq!(
            choose_source(&dom, &seen, None).unwrap(),
            "https://cdn.test/asset/current.m3u8"
        );
        assert!(choose_source(&json!({"sources":[[seen[0]],[seen[1]]]}), &seen, None).is_err());
        assert!(choose_source(&Value::Null, &seen, None).is_err());
        let old = crate::m3u8::direct_source("https://old.test/asset/current.m3u8?sig=expired");
        assert_eq!(
            choose_source(&dom, &seen, Some(&old)).unwrap(),
            "https://cdn.test/asset/current.m3u8"
        );
        assert!(choose_source(&Value::Null, &seen, Some(&old)).is_err());
        let non_hls = json!({"sources":[["https://site.test/main.mp4"]]});
        assert!(choose_source(&non_hls, &seen, None).is_err());
        let api = json!({"sources":[["https://cdn.test/playlist?video=one&sig=fresh"]],"hlsSources":["https://cdn.test/playlist?video=one&sig=fresh"]});
        assert!(choose_source(
            &api,
            &[],
            Some(&crate::m3u8::direct_source(
                "https://old.test/playlist?video=one&sig=expired"
            ))
        )
        .is_ok());
        assert!(choose_source(
            &api,
            &[],
            Some(&crate::m3u8::direct_source(
                "https://old.test/playlist?video=another&sig=expired"
            ))
        )
        .is_err());
    }
    #[test]
    fn browser_cookie_import_preserves_domain_path_and_session_expiry() {
        let cookies = browser_cookies("# Netscape\n#HttpOnly_.cdn.test\tTRUE\t/private\tTRUE\t0\tsid\tsecret\ncdn.test\tFALSE\t/\tFALSE\t1\told\tx");
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0]["domain"], ".cdn.test");
        assert_eq!(cookies[0]["path"], "/private");
        assert!(cookies[0].get("expires").is_none());
    }
}
