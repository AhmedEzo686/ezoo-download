//! Parsing of HLS playlists (.m3u8).

use anyhow::{anyhow, bail};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub uri: String,
    pub iv: Option<[u8; 16]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub url: String,
    pub sequence: u64,
    pub key: Option<Key>,
}

#[derive(Debug, Default, PartialEq)]
pub struct MediaPlaylist {
    pub init_url: Option<String>,
    pub parts: Vec<Part>,
    pub ended: bool,
}

/// Parse `KEY=VALUE,KEY="quoted,value"` attribute lists.
pub fn parse_attrs(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut key = String::new();
    let mut val = String::new();
    let mut in_key = true;
    let mut quoted = false;
    for c in s.chars() {
        if in_key {
            if c == '=' {
                in_key = false;
            } else if c != ',' {
                key.push(c);
            }
        } else if c == '"' {
            quoted = !quoted;
        } else if c == ',' && !quoted {
            out.insert(key.trim().to_ascii_uppercase(), val.clone());
            key.clear();
            val.clear();
            in_key = true;
        } else {
            val.push(c);
        }
    }
    if !key.trim().is_empty() {
        out.insert(key.trim().to_ascii_uppercase(), val);
    }
    out
}

fn resolve(base: &str, rel: &str) -> anyhow::Result<String> {
    let base = url::Url::parse(base)?;
    Ok(base.join(rel.trim())?.to_string())
}

/// For a master playlist, return the URL of the highest quality variant.
pub fn best_variant(base: &str, text: &str) -> anyhow::Result<Option<String>> {
    let mut best: Option<(u64, String)> = None;
    let mut pending_bw: Option<u64> = None;
    for line in text.lines().map(str::trim) {
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let a = parse_attrs(attrs);
            pending_bw = Some(a.get("BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0));
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some(bw) = pending_bw.take() {
                if best.as_ref().map(|(b, _)| bw > *b).unwrap_or(true) {
                    best = Some((bw, resolve(base, line)?));
                }
            }
        }
    }
    Ok(best.map(|(_, u)| u))
}

fn parse_iv(s: &str) -> Option<[u8; 16]> {
    let hex = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// A real stream has a few thousand parts; a list far beyond this is an
/// attack on memory and disk, not a video.
pub const MAX_PARTS: usize = 200_000;
pub const MAX_PLAYLIST_BYTES: usize = 32 << 20;

pub fn parse_media(base: &str, text: &str) -> anyhow::Result<MediaPlaylist> {
    if text.len() > MAX_PLAYLIST_BYTES {
        bail!("the stream's list is too big");
    }
    let mut pl = MediaPlaylist::default();
    let mut seq: u64 = 0;
    let mut key: Option<Key> = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(v) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = v.trim().parse().unwrap_or(0);
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            let a = parse_attrs(attrs);
            let method = a.get("METHOD").map(|s| s.to_ascii_uppercase()).unwrap_or_default();
            key = match method.as_str() {
                "NONE" | "" => None,
                "AES-128" => {
                    let uri = a.get("URI").ok_or_else(|| anyhow!("AES-128 key without URI"))?;
                    Some(Key {
                        uri: resolve(base, uri)?,
                        iv: a.get("IV").and_then(|v| parse_iv(v)),
                    })
                }
                other => bail!("this stream is DRM protected ({other}) and cannot be downloaded"),
            };
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            let a = parse_attrs(attrs);
            if let Some(uri) = a.get("URI") {
                pl.init_url = Some(resolve(base, uri)?);
            }
        } else if line.starts_with("#EXT-X-ENDLIST") {
            pl.ended = true;
        } else if !line.starts_with('#') {
            if pl.parts.len() >= MAX_PARTS {
                bail!("the stream has too many parts");
            }
            pl.parts.push(Part {
                url: resolve(base, line)?,
                sequence: seq,
                key: key.clone(),
            });
            seq = seq.wrapping_add(1);
        }
    }
    Ok(pl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_lists_are_refused() {
        let mut huge = String::from("#EXTM3U\n");
        for i in 0..=MAX_PARTS {
            huge.push_str(&format!("p{i}.ts\n"));
        }
        assert!(parse_media("https://x.example/a/list.m3u8", &huge).is_err());
        assert!(parse_media("https://x.example/a/list.m3u8", &"#".repeat(MAX_PLAYLIST_BYTES + 1)).is_err());
        assert!(parse_media("https://x.example/a/list.m3u8", "#EXTM3U\np1.ts\n").is_ok());
    }

    #[test]
    fn attrs_with_quotes() {
        let a = parse_attrs("METHOD=AES-128,URI=\"https://k/key?a=1,2\",IV=0x00000000000000000000000000000001");
        assert_eq!(a["METHOD"], "AES-128");
        assert_eq!(a["URI"], "https://k/key?a=1,2");
        assert_eq!(parse_iv(&a["IV"]).unwrap()[15], 1);
    }

    #[test]
    fn master_picks_highest_bandwidth() {
        let m = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nhigh/index.m3u8\n";
        let u = best_variant("https://cdn.x/v/master.m3u8", m).unwrap().unwrap();
        assert_eq!(u, "https://cdn.x/v/high/index.m3u8");
    }

    #[test]
    fn media_playlist() {
        let m = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:4,\na.ts\n#EXTINF:4,\nb.ts\n#EXT-X-ENDLIST\n";
        let p = parse_media("https://cdn.x/v/index.m3u8", m).unwrap();
        assert!(p.ended);
        assert_eq!(p.parts.len(), 2);
        assert_eq!(p.parts[1].url, "https://cdn.x/v/b.ts");
        assert_eq!(p.parts[1].sequence, 8);
        assert_eq!(p.parts[0].key.as_ref().unwrap().uri, "https://cdn.x/v/key.bin");
    }

    #[test]
    fn drm_is_refused() {
        let m = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\na.ts\n#EXT-X-ENDLIST\n";
        assert!(parse_media("https://cdn.x/index.m3u8", m).is_err());
    }
}
