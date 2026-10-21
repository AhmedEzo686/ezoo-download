//! Download lists in the .ef2 list format, so lists move between
//! Ezoo Download and other download managers:
//!
//! ```text
//! <
//! https://example.com/file.pdf
//! referer: https://example.com/
//! User-Agent: Mozilla/5.0 ...
//! >
//! ```

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ef2Item {
    pub url: String,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub file_name: Option<String>,
    pub description: Option<String>,
}

pub fn parse(text: &str) -> Vec<Ef2Item> {
    let mut out = Vec::new();
    let mut cur: Option<Ef2Item> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line == "<" {
            cur = Some(Ef2Item::default());
            continue;
        }
        if line == ">" {
            if let Some(item) = cur.take().filter(|i| !i.url.is_empty()) {
                out.push(item);
            }
            continue;
        }
        let Some(item) = cur.as_mut() else {
            // a plain text list: one address per line
            if line.starts_with("http://") || line.starts_with("https://") {
                out.push(Ef2Item { url: line.to_string(), ..Default::default() });
            }
            continue;
        };
        if item.url.is_empty() && (line.starts_with("http://") || line.starts_with("https://") || line.starts_with("ftp://")) {
            item.url = line.to_string();
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().to_string();
            if value.is_empty() {
                continue;
            }
            match key.trim().to_ascii_lowercase().as_str() {
                "referer" => item.referer = Some(value),
                "cookie" => item.cookies = Some(value),
                "user-agent" => item.user_agent = Some(value),
                "filename" => item.file_name = Some(value),
                "description" => item.description = Some(value),
                _ => {}
            }
        }
    }
    out
}

pub fn write(items: &[Ef2Item]) -> String {
    let mut s = String::new();
    for i in items {
        s.push_str("<\r\n");
        s.push_str(&i.url);
        s.push_str("\r\n");
        if let Some(r) = &i.referer {
            s.push_str(&format!("referer: {r}\r\n"));
        }
        if let Some(c) = &i.cookies {
            s.push_str(&format!("cookie: {c}\r\n"));
        }
        if let Some(u) = &i.user_agent {
            s.push_str(&format!("User-Agent: {u}\r\n"));
        }
        if let Some(n) = &i.file_name {
            s.push_str(&format!("filename: {n}\r\n"));
        }
        if let Some(d) = &i.description {
            s.push_str(&format!("description: {d}\r\n"));
        }
        s.push_str(">\r\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        let items = vec![
            Ef2Item {
                url: "https://example.com/x.pdf".into(),
                referer: Some("https://example.com/".into()),
                user_agent: Some("UA/1".into()),
                ..Default::default()
            },
            Ef2Item { url: "https://example.org/y.mp4".into(), ..Default::default() },
        ];
        assert_eq!(parse(&write(&items)), items);
        let plain = parse("https://example.net/1.pdf\nnot a link\nhttps://example.net/2.pdf");
        assert_eq!(plain.len(), 2);
    }
}
