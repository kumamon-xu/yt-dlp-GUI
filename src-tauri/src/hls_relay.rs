//! Task-scoped, loopback-only resource service. URLs/credentials are held in
//! memory and are never accepted from HTTP clients. Not a general web proxy.
use crate::hls_preflight::RequestContext;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

const SEGMENT_LIMIT: usize = 32 * 1024 * 1024;
pub struct Relay {
    origin: String,
    token: String,
    resources: Arc<Mutex<BTreeMap<String, Url>>>,
    worker: tokio::task::JoinHandle<()>,
}
impl std::fmt::Debug for Relay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalHlsService")
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl Relay {
    pub fn register(&self, url: Url) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let id = format!("{:x}", Sha256::digest(url.as_str().as_bytes()));
        let mut resources = self.resources.lock().map_err(|_| "HLS 资源服务状态异常")?;
        if resources.len() >= 100_000 && !resources.contains_key(&id) {
            return Err("HLS 资源服务数量超过上限".into());
        }
        resources.insert(id.clone(), url);
        Ok(format!("{}/{}/{id}", self.origin, self.token))
    }
}

pub async fn start(context: RequestContext) -> Result<Arc<Relay>, String> {
    use sha2::{Digest, Sha256};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "无法启动本机 HLS 资源服务")?;
    let origin = format!(
        "http://{}",
        listener.local_addr().map_err(|_| "无法读取 HLS 服务地址")?
    );
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).map_err(|_| "无法生成 HLS 服务随机标识")?;
    let token = format!("{:x}", Sha256::digest(nonce));
    let resources = Arc::new(Mutex::new(BTreeMap::new()));
    let (map, key) = (resources.clone(), token.clone());
    let worker = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept(), if clients.len() < 16 => {
                    let Ok((socket,_)) = accepted else { break; };
                    let (context,map,key) = (context.clone(),map.clone(),key.clone());
                    clients.spawn(async move { let _ = tokio::time::timeout(Duration::from_secs(50),serve(socket,context,map,key)).await; });
                }
                _ = clients.join_next(), if !clients.is_empty() => {}
            }
        }
    });
    Ok(Arc::new(Relay {
        origin,
        token,
        resources,
        worker,
    }))
}

fn parse_range(value: &str) -> Option<(u64, u64)> {
    let (start, end) = value.strip_prefix("bytes=")?.trim().split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    let end = end.parse::<u64>().ok()?;
    let length = end.checked_sub(start)?.checked_add(1)?;
    (length <= SEGMENT_LIMIT as u64).then_some((start, length))
}
async fn serve(
    mut socket: tokio::net::TcpStream,
    context: RequestContext,
    resources: Arc<Mutex<BTreeMap<String, Url>>>,
    token: String,
) {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let Ok(size) = socket.read(&mut buf).await else {
            return;
        };
        if size == 0 || request.len() + size > 8192 {
            return;
        }
        request.extend_from_slice(&buf[..size]);
    }
    let Ok(text) = std::str::from_utf8(&request) else {
        return;
    };
    let mut lines = text.lines();
    let first: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    if first.len() != 3 || first[0] != "GET" || !first[2].starts_with("HTTP/1.") {
        return;
    }
    let prefix = format!("/{token}/");
    let url = first[1]
        .strip_prefix(&prefix)
        .and_then(|id| resources.lock().ok()?.get(id).cloned());
    let Some(url) = url else {
        let _ = socket
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    };
    let mut range = None;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("range") {
                let Some(parsed) = parse_range(v.trim()) else {
                    let _=socket.write_all(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                };
                range = Some(parsed);
            }
        }
    }
    let limit = range.map(|r| r.1 as usize).unwrap_or(SEGMENT_LIMIT);
    match context.get(&url, range, limit, range.is_none()).await {
        Ok((_, data)) => {
            // Each registered URL is one resource. RE consumes exactly the body;
            // range requests contain only the validated requested byte span.
            let (status, range_header) = if let Some((offset, _)) = range {
                (
                    "206 Partial Content",
                    format!(
                        "Content-Range: bytes {offset}-{}/*\r\n",
                        offset + data.len() as u64 - 1
                    ),
                )
            } else {
                ("200 OK", String::new())
            };
            let header = format!("HTTP/1.1 {status}\r\n{range_header}Content-Type: application/octet-stream\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",data.len());
            if socket.write_all(header.as_bytes()).await.is_ok() {
                let _ = socket.write_all(&data).await;
            }
        }
        Err(error) => {
            context.report_resource_failure(&url, &error.reason);
            let _ = socket
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_bounded_single_byte_ranges_are_accepted() {
        assert_eq!(parse_range("bytes=188-375"), Some((188, 188)));
        for value in [
            "bytes=0-",
            "bytes=-2",
            "bytes=5-2",
            "bytes=0-1,5-6",
            "bytes=0-999999999",
            "garbage",
        ] {
            assert!(parse_range(value).is_none());
        }
    }
    #[tokio::test]
    async fn unregistered_resources_are_rejected_and_drop_stops_service() {
        let source = crate::m3u8::direct_source("https://media.test/index.m3u8");
        let relay = start(RequestContext::new(&Default::default(), &source, None).unwrap())
            .await
            .unwrap();
        let url = relay
            .register(Url::parse("https://media.test/asset/a.ts?token=secret").unwrap())
            .unwrap();
        assert!(!url.contains("secret") && !url.contains("media.test"));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        assert_eq!(
            client
                .get(format!("{}/missing", relay.origin))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        let origin = relay.origin.clone();
        drop(relay);
        tokio::task::yield_now().await;
        assert!(client.get(origin).send().await.is_err());
    }
}
