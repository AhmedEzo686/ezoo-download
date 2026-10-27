//! Working out a good, safe file name for a download.

use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};

/// Extension used while a file is still downloading.
pub const PART_EXT: &str = "sgpart";

/// Read the file name out of a `Content-Disposition` header.
/// Handles both `filename="a.zip"` and RFC 5987 `filename*=UTF-8''a%20b.zip`.
pub fn from_content_disposition(value: &str) -> Option<String> {
    let mut plain: Option<String> = None;
    let mut extended: Option<String> = None;
    for part in value.split(';') {
        let part = part.trim();
        let Some((key, val)) = part.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim().trim_matches('"');
        if key == "filename*" {
            // charset'lang'encoded-value
            let encoded = val.splitn(3, '\'').nth(2).unwrap_or(val);
            extended = Some(percent_decode_str(encoded).decode_utf8_lossy().into_owned());
        } else if key == "filename" {
            let decoded = if val.contains('%') {
                percent_decode_str(val).decode_utf8_lossy().into_owned()
            } else {
                val.to_string()
            };
            plain = Some(decoded);
        }
    }
    extended.or(plain).filter(|s| !s.trim().is_empty())
}

/// Last path segment of a URL, percent-decoded.
pub fn from_url(u: &str) -> Option<String> {
    let parsed = url::Url::parse(u).ok()?;
    let last = parsed
        .path_segments()?
        .rfind(|s| !s.is_empty())?
        .to_string();
    let name = percent_decode_str(&last).decode_utf8_lossy().into_owned();
    if name.trim().is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Make a name valid on Windows (the strictest file system we target).
pub fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 32 => '_',
            c => c,
        })
        .collect();
    // no trailing dots or spaces on Windows
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        return "download".into();
    }
    let stem_upper = out
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let out = if RESERVED.contains(&stem_upper.as_str()) {
        format!("_{out}")
    } else {
        out
    };
    truncate_keep_ext(&out, 180)
}

fn truncate_keep_ext(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    let keep = max_chars.saturating_sub(ext.chars().count() + 1).max(1);
    let stem: String = stem.chars().take(keep).collect();
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

/// ("archive.tar", "gz") for "archive.tar.gz"; ("name", "") without extension.
pub fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

pub fn extension(name: &str) -> String {
    split_ext(name).1.to_ascii_lowercase()
}

/// Add an extension guessed from the MIME type when the name has none.
pub fn ensure_extension(name: String, content_type: Option<&str>) -> String {
    if !split_ext(&name).1.is_empty() {
        return name;
    }
    let mime = content_type
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let ext = match mime.as_str() {
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/mp2t" => "ts",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/ogg" => "ogg",
        "application/pdf" => "pdf",
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/x-rar-compressed" | "application/vnd.rar" => "rar",
        "application/x-7z-compressed" => "7z",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
        "application/x-msi" => "msi",
        "application/vnd.android.package-archive" => "apk",
        "application/x-iso9660-image" => "iso",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "text/plain" => "txt",
        "application/vnd.apple.mpegurl" | "application/x-mpegurl" => "ts",
        _ => "",
    };
    if ext.is_empty() {
        name
    } else {
        format!("{name}.{ext}")
    }
}

/// Pick a path in `dir` that is not taken yet by a finished file or a
/// partial download: "file.zip", "file (1).zip", "file (2).zip", ...
pub fn unique_name(dir: &Path, name: &str) -> String {
    let taken = |n: &str| {
        dir.join(n).exists() || dir.join(format!("{n}.{PART_EXT}")).exists()
    };
    if !taken(name) {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    for i in 1..10_000 {
        let candidate = if ext.is_empty() {
            format!("{stem} ({i})")
        } else {
            format!("{stem} ({i}).{ext}")
        };
        if !taken(&candidate) {
            return candidate;
        }
    }
    name.to_string()
}

pub fn part_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{PART_EXT}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_variants() {
        assert_eq!(
            from_content_disposition("attachment; filename=\"report.pdf\"").as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            from_content_disposition("attachment; filename=a.zip; filename*=UTF-8''%D9%85%D9%84%D9%81.zip")
                .as_deref(),
            Some("ملف.zip")
        );
        assert_eq!(from_content_disposition("inline"), None);
    }

    #[test]
    fn url_names() {
        assert_eq!(
            from_url("https://example.com/files/My%20File.pdf?token=1").as_deref(),
            Some("My File.pdf")
        );
        assert_eq!(from_url("https://x.com/"), None);
    }

    #[test]
    fn sanitizing() {
        assert_eq!(sanitize("a:b*c?.zip"), "a_b_c_.zip");
        assert_eq!(sanitize("CON.txt"), "_CON.txt");
        assert_eq!(sanitize("name. . "), "name");
        assert_eq!(sanitize(""), "download");
    }

    #[test]
    fn extensions() {
        assert_eq!(split_ext("a.tar.gz"), ("a.tar", "gz"));
        assert_eq!(split_ext(".hidden"), (".hidden", ""));
        assert_eq!(ensure_extension("video".into(), Some("video/mp4; x=y")), "video.mp4");
        assert_eq!(ensure_extension("a.bin".into(), Some("video/mp4")), "a.bin");
    }
}
