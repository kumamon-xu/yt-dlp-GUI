//! Discover alternatives from provider declarations or bounded site adapters.
//! Candidates are transient and must pass the regular VOD checks before use.

use crate::hls_preflight::{attributes, http_url, MirrorRule, RequestContext};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;
use url::Url;

pub type Alternatives = BTreeMap<String, Vec<Url>>;
const MAX_STREAMS: usize = 128;
const MAX_ALTERNATIVES: usize = 8;

#[derive(Clone)]
struct Stream {
    url: Url,
    original_url: Url,
    attrs: BTreeMap<String, String>,
    rendition: bool,
    pathway: String,
}

fn streams(lines: &[String], base: &Url) -> Vec<Stream> {
    let mut result = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let rendition = line.starts_with("#EXT-X-MEDIA:");
        if !rendition && !line.starts_with("#EXT-X-STREAM-INF:") {
            continue;
        }
        let attrs = attributes(line);
        let raw = if rendition {
            attrs.get("URI").map(String::as_str)
        } else {
            lines[i + 1..]
                .iter()
                .find(|s| !s.is_empty() && !s.starts_with('#'))
                .map(String::as_str)
        };
        let Some(url) = raw
            .and_then(|r| base.join(r).ok())
            .filter(|u| http_url(u.as_str()).is_ok())
        else {
            continue;
        };
        let pathway = attrs
            .get("PATHWAY-ID")
            .cloned()
            .unwrap_or_else(|| ".".into());
        result.push(Stream {
            original_url: url.clone(),
            url,
            attrs,
            rendition,
            pathway,
        });
        if result.len() == MAX_STREAMS {
            break;
        }
    }
    // Renditions belong to the pathways whose variants reference their groups.
    let variants = result.clone();
    let mut expanded = Vec::new();
    for s in result {
        if !s.rendition {
            expanded.push(s);
            continue;
        }
        let kind = s.attrs.get("TYPE").map(String::as_str).unwrap_or("");
        for v in variants
            .iter()
            .filter(|v| !v.rendition && v.attrs.get(kind) == s.attrs.get("GROUP-ID"))
        {
            let mut r = s.clone();
            r.pathway = v.pathway.clone();
            expanded.push(r);
            if expanded.len() >= MAX_STREAMS {
                return expanded;
            }
        }
    }
    expanded
}

fn equivalent(a: &Stream, b: &Stream) -> bool {
    if a.rendition != b.rendition {
        return false;
    }
    // A provider explicitly clones this exact stream, even without a stable ID.
    if b.url != b.original_url && a.url == b.original_url {
        return true;
    }
    let id = if a.rendition {
        "STABLE-RENDITION-ID"
    } else {
        "STABLE-VARIANT-ID"
    };
    // Resolution alone cannot establish identity (different fps, codecs or angles).
    if a.attrs.get(id).is_none_or(String::is_empty) || a.attrs.get(id) != b.attrs.get(id) {
        return false;
    }
    let fields: &[&str] = if a.rendition {
        &[
            "TYPE",
            "LANGUAGE",
            "ASSOC-LANGUAGE",
            "NAME",
            "CHANNELS",
            "CHARACTERISTICS",
            "FORCED",
        ]
    } else {
        &[
            "BANDWIDTH",
            "AVERAGE-BANDWIDTH",
            "RESOLUTION",
            "CODECS",
            "FRAME-RATE",
            "VIDEO-RANGE",
            "HDCP-LEVEL",
        ]
    };
    fields.iter().all(|k| a.attrs.get(*k) == b.attrs.get(*k))
}

fn add(map: &mut Alternatives, from: &Url, to: &Url) {
    if from == to || (from.scheme() == "https" && to.scheme() != "https") {
        return;
    }
    let entry = map.entry(from.to_string()).or_default();
    if entry.len() < MAX_ALTERNATIVES && !entry.contains(to) {
        entry.push(to.clone());
    }
}

fn clone_url(stream: &Stream, replacement: &Value) -> Option<Url> {
    let id = if stream.rendition {
        "STABLE-RENDITION-ID"
    } else {
        "STABLE-VARIANT-ID"
    };
    let overrides = if stream.rendition {
        "PER-RENDITION-URIS"
    } else {
        "PER-VARIANT-URIS"
    };
    if let Some(raw) = stream
        .attrs
        .get(id)
        .and_then(|id| replacement[overrides][id].as_str())
    {
        let u = http_url(raw).ok()?;
        return (!(stream.url.scheme() == "https" && u.scheme() != "https")).then_some(u);
    }
    let mut u = stream.url.clone();
    if let Some(host) = replacement["HOST"].as_str() {
        let target = http_url(&format!("{}://{host}/", u.scheme())).ok()?;
        if target.path() != "/" || target.query().is_some() || target.fragment().is_some() {
            return None;
        }
        u.set_host(target.host_str()).ok()?;
        u.set_port(target.port()).ok()?;
    }
    if let Some(params) = replacement["PARAMS"].as_object().filter(|p| !p.is_empty()) {
        if params.len() > 32 || params.values().any(|v| !v.is_string()) {
            return None;
        }
        // Keep untouched query octets; signed parameters must not be re-encoded.
        let retained: Vec<_> = u
            .query()
            .unwrap_or("")
            .split('&')
            .filter(|s| !s.is_empty())
            .filter(|s| {
                let key = url::form_urlencoded::parse(s.as_bytes())
                    .next()
                    .map(|p| p.0.into_owned());
                key.is_none_or(|k| !params.contains_key(&k))
            })
            .map(String::from)
            .collect();
        let ordered: BTreeMap<_, _> = params.iter().collect();
        let mut encoded = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in ordered {
            encoded.append_pair(key, value.as_str()?);
        }
        let mut parts = retained;
        parts.push(encoded.finish());
        u.set_query(Some(&parts.join("&")));
    }
    Some(u)
}

fn declared(lines: &[String], base: &Url, steering: Option<&Value>) -> Alternatives {
    let original = streams(lines, base);
    let mut all = original.clone();
    if let Some(clones) = steering.and_then(|v| v["PATHWAY-CLONES"].as_array()) {
        for clone in clones.iter().take(MAX_ALTERNATIVES) {
            let (Some(base_id), Some(id)) = (clone["BASE-ID"].as_str(), clone["ID"].as_str())
            else {
                continue;
            };
            if id == base_id || all.iter().any(|s| s.pathway == id) {
                continue;
            }
            let copies: Vec<_> = all
                .iter()
                .filter(|s| s.pathway == base_id)
                .cloned()
                .collect();
            for mut s in copies {
                let Some(url) = clone_url(&s, &clone["URI-REPLACEMENT"]) else {
                    continue;
                };
                s.url = url;
                s.pathway = id.into();
                all.push(s);
                if all.len() >= MAX_STREAMS * 2 {
                    break;
                }
            }
        }
    }
    let priority = steering.and_then(|v| v["PATHWAY-PRIORITY"].as_array());
    all.sort_by_key(|s| {
        priority
            .and_then(|p| p.iter().position(|id| id.as_str() == Some(&s.pathway)))
            .unwrap_or(usize::MAX)
    });
    let mut result = Alternatives::new();
    for a in &original {
        for b in &all {
            if equivalent(a, b) {
                add(&mut result, &a.url, &b.url);
            }
        }
    }
    result
}

pub async fn from_master(ctx: &RequestContext, lines: &[String], base: &Url) -> Alternatives {
    let steering_uri = lines
        .iter()
        .find(|s| s.starts_with("#EXT-X-CONTENT-STEERING:"))
        .and_then(|s| attributes(s).remove("SERVER-URI"))
        .and_then(|raw| base.join(&raw).ok())
        .filter(|u| {
            http_url(u.as_str()).is_ok() && !(base.scheme() == "https" && u.scheme() != "https")
        });
    let steering = if let Some(uri) = steering_uri {
        match tokio::time::timeout(
            Duration::from_secs(5),
            ctx.get(&uri, None, 128 * 1024, true),
        )
        .await
        {
            Ok(Ok((_, bytes))) => serde_json::from_slice::<Value>(&bytes)
                .ok()
                .filter(|v| v["VERSION"] == 1),
            _ => None, // Steering availability never prevents a healthy original download.
        }
    } else {
        None
    };
    declared(lines, base, steering.as_ref())
}

/// This adapter is limited to the site's known root mirror and the current asset
/// directory. Sharing a DNS suffix is never a generic reason to trust a mirror.
pub fn site_rule(manifest: &Url, resource: &Url) -> Option<MirrorRule> {
    let host = manifest.host_str()?;
    let resource_host = resource.host_str()?;
    let site_host = |h: &str| h == "javday.homes" || h.ends_with(".javday.homes");
    let prefix = manifest.path().rsplit_once('/')?.0.to_string() + "/";
    if manifest.scheme() != "https"
        || resource.scheme() != "https"
        || manifest.port_or_known_default() != Some(443)
        || resource.port_or_known_default() != Some(443)
        || !site_host(host)
        || !site_host(resource_host)
        || resource_host == "javday.homes"
        || !prefix.starts_with("/videos/")
        || prefix == "/videos/"
        || !resource.path().starts_with(&prefix)
    {
        return None;
    }
    Some(MirrorRule {
        from_origin: resource.origin().ascii_serialization(),
        to_origin: "https://javday.homes".into(),
        path_prefix: prefix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn site_adapter_is_asset_scoped_and_does_not_guess_other_sites() {
        let m = Url::parse("https://javday.homes/videos/id/quality/index.m3u8").unwrap();
        let r = Url::parse("https://random.javday.homes/videos/id/quality/a.ts?sig=a%2Fb").unwrap();
        let rule = site_rule(&m, &r).unwrap();
        assert_eq!(
            rule.candidate(&r).unwrap().as_str(),
            "https://javday.homes/videos/id/quality/a.ts?sig=a%2Fb"
        );
        for raw in [
            "https://javday.homes.evil.test/videos/id/quality/a.ts",
            "https://random.javday.homes/videos/other/a.ts",
            "http://random.javday.homes/videos/id/quality/a.ts",
            "https://random.javday.homes:8443/videos/id/quality/a.ts",
        ] {
            assert!(site_rule(&m, &Url::parse(raw).unwrap()).is_none());
        }
        assert!(site_rule(
            &Url::parse("https://unknown.test/videos/id/quality/index.m3u8").unwrap(),
            &r
        )
        .is_none());
    }

    #[test]
    fn declared_identity_preserves_codec_fps_and_audio_language() {
        let text = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"English\",LANGUAGE=\"en\",STABLE-RENDITION-ID=\"audio\",URI=\"a.m3u8\"\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"b\",NAME=\"English\",LANGUAGE=\"en\",STABLE-RENDITION-ID=\"audio\",URI=\"backup-a.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=100,RESOLUTION=1280x720,CODECS=\"avc1\",FRAME-RATE=30,STABLE-VARIANT-ID=\"v\",PATHWAY-ID=\"a\",AUDIO=\"a\"\nv.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=100,RESOLUTION=1280x720,CODECS=\"avc1\",FRAME-RATE=30,STABLE-VARIANT-ID=\"v\",PATHWAY-ID=\"b\",AUDIO=\"b\"\nbackup-v.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=100,RESOLUTION=1280x720,CODECS=\"hev1\",FRAME-RATE=60,STABLE-VARIANT-ID=\"v\"\nwrong.m3u8";
        let base = Url::parse("https://site.test/master.m3u8").unwrap();
        let lines: Vec<_> = text.lines().map(String::from).collect();
        let map = declared(&lines, &base, None);
        assert_eq!(
            map["https://site.test/v.m3u8"],
            vec![base.join("backup-v.m3u8").unwrap()]
        );
        assert_eq!(
            map["https://site.test/a.m3u8"],
            vec![base.join("backup-a.m3u8").unwrap()]
        );
        assert!(!map.contains_key("https://site.test/wrong.m3u8"));
        let no_ids: Vec<_> = lines
            .iter()
            .map(|s| s.replace(",STABLE-VARIANT-ID=\"v\"", ""))
            .collect();
        assert!(!declared(&no_ids, &base, None).contains_key("https://site.test/v.m3u8"));
    }

    #[test]
    fn steering_clones_preserve_signed_octets_and_apply_per_stream_overrides() {
        let base = Url::parse("https://origin.test/master.m3u8").unwrap();
        let lines = vec![
            "#EXT-X-STREAM-INF:BANDWIDTH=100,PATHWAY-ID=\"a\",STABLE-VARIANT-ID=\"v\"".into(),
            "video.m3u8?sig=a%2Fb%3D&token=old&x=1&x=2".into(),
        ];
        let value = json!({"PATHWAY-PRIORITY":["b"],"PATHWAY-CLONES":[{"BASE-ID":"a","ID":"b","URI-REPLACEMENT":{"HOST":"backup.test:8443","PARAMS":{"token":"new space"}}}]});
        let map = declared(&lines, &base, Some(&value));
        assert_eq!(
            map.values().next().unwrap()[0].as_str(),
            "https://backup.test:8443/video.m3u8?sig=a%2Fb%3D&x=1&x=2&token=new+space"
        );
        let override_value = json!({"PATHWAY-CLONES":[{"BASE-ID":"a","ID":"b","URI-REPLACEMENT":{"PER-VARIANT-URIS":{"v":"https://different.test/new.m3u8?sig=exact%2F"}}}]});
        assert_eq!(
            declared(&lines, &base, Some(&override_value))
                .values()
                .next()
                .unwrap()[0]
                .as_str(),
            "https://different.test/new.m3u8?sig=exact%2F"
        );
        let downgrade = json!({"PATHWAY-CLONES":[{"BASE-ID":"a","ID":"b","URI-REPLACEMENT":{"PER-VARIANT-URIS":{"v":"http://different.test/new.m3u8"}}}]});
        assert!(declared(&lines, &base, Some(&downgrade)).is_empty());
    }
}
