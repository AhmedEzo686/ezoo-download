//! The command line: other programs and scripts hand downloads over.
//!
//! ```text
//! EzooDownload.exe /d <address> [/p <folder>] [/f <file name>] [/a] [/n] [/s]
//!   /d  the address to download (http, https, ftp, ftps)
//!   /p  the folder to save it in
//!   /f  the file name to save it under
//!   /a  only put it in the main queue (not started)
//!   /n  no window: straight to the list
//!   /s  start the main queue
//! ```
//! "-d" works as "/d". Nothing on the command line is ever run: an address
//! must be a web or FTP address, a folder and a name are only used as such.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CliAsk {
    pub url: Option<String>,
    pub folder: Option<String>,
    pub name: Option<String>,
    /// only into the main queue
    pub later: bool,
    /// no "Download File Info" window
    pub quiet: bool,
    /// start the main queue
    pub start_queue: bool,
}

impl CliAsk {
    pub fn is_empty(&self) -> bool {
        self.url.is_none() && !self.start_queue
    }
}

/// The switches, from the program's arguments (the program's own path not
/// included). None: no switch of the command line at all.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Option<CliAsk> {
    let mut ask = CliAsk::default();
    let mut any = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let key = a.trim().to_ascii_lowercase();
        let switch = key.strip_prefix('/').or_else(|| key.strip_prefix('-').filter(|k| !k.starts_with('-')));
        let Some(switch) = switch else { continue };
        match switch {
            "d" => {
                any = true;
                ask.url = it.next().map(|u| u.trim().to_string()).filter(|u| crate::is_download_address(u) && url::Url::parse(u).is_ok());
            }
            "p" => {
                any = true;
                ask.folder = it.next().map(|f| f.trim().trim_matches('"').to_string()).filter(|f| !f.is_empty());
            }
            "f" => {
                any = true;
                ask.name = it
                    .next()
                    .map(|n| swiftget_engine::filename::sanitize(n.trim().trim_matches('"')))
                    .filter(|n| !n.is_empty());
            }
            "a" => {
                any = true;
                ask.later = true;
            }
            "n" => {
                any = true;
                ask.quiet = true;
            }
            "s" => {
                any = true;
                ask.start_queue = true;
            }
            _ => {}
        }
    }
    any.then_some(ask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Option<CliAsk> {
        parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn switches() {
        let a = p(&["/d", "https://a.test/f.zip", "/p", "D:\\Files", "/f", "new.zip", "/n", "/a"]).unwrap();
        assert_eq!(a.url.as_deref(), Some("https://a.test/f.zip"));
        assert_eq!(a.folder.as_deref(), Some("D:\\Files"));
        assert_eq!(a.name.as_deref(), Some("new.zip"));
        assert!(a.quiet && a.later && !a.start_queue);
        let b = p(&["-d", "ftp://h.test/x.iso", "-s"]).unwrap();
        assert!(b.start_queue && b.url.is_some());
        // not an address: nothing is added (never a program, never a file of the computer)
        assert_eq!(p(&["/d", "C:\\Windows\\x.exe"]).unwrap().url, None);
        assert_eq!(p(&["/d", "javascript:alert(1)"]).unwrap().url, None);
        // a name can't lead out of the folder
        let name = p(&["/d", "https://a.test/f", "/f", "..\\..\\evil.exe"]).unwrap().name.unwrap();
        assert!(!name.contains('\\') && !name.contains('/'), "{name}");
        // the program's own switches are none of these
        assert_eq!(p(&["--background"]), None);
        assert_eq!(p(&["C:\\x.torrent"]), None);
    }
}
