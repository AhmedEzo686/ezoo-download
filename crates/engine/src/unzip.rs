//! Unpacking a .zip archive (yt-dlp's folder build), only what is needed:
//! stored and deflated files, no archive larger than 4 GB. Every name is
//! checked: nothing is ever written outside the folder given, and every
//! file's CRC must match.

use anyhow::{anyhow, bail, Context};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

fn u16_at(b: &[u8], at: usize) -> anyhow::Result<u16> {
    b.get(at..at + 2).map(|x| u16::from_le_bytes([x[0], x[1]])).ok_or_else(|| anyhow!("the archive is cut short"))
}

fn u32_at(b: &[u8], at: usize) -> anyhow::Result<u32> {
    b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).ok_or_else(|| anyhow!("the archive is cut short"))
}

/// A name of the archive as a path inside `dest`; None for a name that
/// leads out of it (absolute, "..", a drive).
fn inside(dest: &Path, name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    let mut out = dest.to_path_buf();
    let mut any = false;
    for c in Path::new(&name).components() {
        match c {
            Component::Normal(p) => {
                let s = p.to_string_lossy();
                if s.contains(':') {
                    return None;
                }
                out.push(p);
                any = true;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    any.then_some(out)
}

/// Unpack all of `zip` (the archive's bytes) into `dest`. `max_total`: the
/// unpacked files together are never bigger.
pub fn unpack(zip: &[u8], dest: &Path, max_total: u64) -> anyhow::Result<usize> {
    // the end of the central directory: the last "PK\5\6" within the last 64 KB
    let from = zip.len().saturating_sub(22 + 65535);
    let end = (from..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| zip[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| anyhow!("not a zip archive"))?;
    let count = u16_at(zip, end + 10)? as usize;
    let dir_size = u32_at(zip, end + 12)? as usize;
    let dir_at = u32_at(zip, end + 16)? as usize;
    if count == 0xFFFF || dir_at == 0xFFFF_FFFF || dir_at.checked_add(dir_size).is_none_or(|e| e > end) {
        bail!("an archive this program can't read");
    }
    std::fs::create_dir_all(dest)?;
    let mut total: u64 = 0;
    let mut at = dir_at;
    let mut files = 0;
    for _ in 0..count {
        if u32_at(zip, at)? != 0x0201_4b50 {
            bail!("the archive's list is damaged");
        }
        let flags = u16_at(zip, at + 8)?;
        let method = u16_at(zip, at + 10)?;
        let crc = u32_at(zip, at + 16)?;
        let packed = u32_at(zip, at + 20)? as usize;
        let size = u32_at(zip, at + 24)? as u64;
        let name_len = u16_at(zip, at + 28)? as usize;
        let extra_len = u16_at(zip, at + 30)? as usize;
        let note_len = u16_at(zip, at + 32)? as usize;
        let local = u32_at(zip, at + 42)? as usize;
        let name = zip.get(at + 46..at + 46 + name_len).ok_or_else(|| anyhow!("the archive is cut short"))?;
        let name = String::from_utf8_lossy(name).into_owned();
        at += 46 + name_len + extra_len + note_len;
        if flags & 1 != 0 {
            bail!("a locked archive");
        }
        let target = inside(dest, &name).ok_or_else(|| anyhow!("a name in the archive leads out of its folder: {name}"))?;
        if name.ends_with('/') || name.ends_with('\\') {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        total += size;
        if total > max_total {
            bail!("the archive unpacks far bigger than expected");
        }
        // the file's own header, then its data
        if u32_at(zip, local)? != 0x0403_4b50 {
            bail!("the archive is damaged");
        }
        let start = local + 30 + u16_at(zip, local + 26)? as usize + u16_at(zip, local + 28)? as usize;
        let data = zip.get(start..start + packed).ok_or_else(|| anyhow!("the archive is cut short"))?;
        let bytes = match method {
            0 => data.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(size as usize);
                flate2::read::DeflateDecoder::new(data).take(size + 1).read_to_end(&mut out).context("a damaged file in the archive")?;
                out
            }
            m => bail!("the archive packs files in a way this program can't read ({m})"),
        };
        if bytes.len() as u64 != size || crc32fast::hash(&bytes) != crc {
            bail!("a damaged file in the archive: {name}");
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &bytes)?;
        files += 1;
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small archive made by hand: one stored file, one deflated.
    fn archive(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        use std::io::Write;
        let mut out = Vec::new();
        let mut dir = Vec::new();
        for (name, body, pack) in entries {
            let data = if *pack {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(body).unwrap();
                e.finish().unwrap()
            } else {
                body.to_vec()
            };
            let crc = crc32fast::hash(body);
            let local = out.len() as u32;
            let method: u16 = if *pack { 8 } else { 0 };
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&data);
            dir.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            dir.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
            dir.extend_from_slice(&method.to_le_bytes());
            dir.extend_from_slice(&[0; 4]);
            dir.extend_from_slice(&crc.to_le_bytes());
            dir.extend_from_slice(&(data.len() as u32).to_le_bytes());
            dir.extend_from_slice(&(body.len() as u32).to_le_bytes());
            dir.extend_from_slice(&(name.len() as u16).to_le_bytes());
            dir.extend_from_slice(&[0; 12]);
            dir.extend_from_slice(&local.to_le_bytes());
            dir.extend_from_slice(name.as_bytes());
        }
        let at = out.len() as u32;
        out.extend_from_slice(&dir);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(dir.len() as u32).to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn unpacks_and_refuses_names_leading_out() {
        let dest = std::env::temp_dir().join(format!("ezoo-unzip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        let big = vec![7u8; 100_000];
        let z = archive(&[("yt-dlp.exe", b"program", false), ("_internal/lib.dll", &big, true)]);
        assert_eq!(unpack(&z, &dest, 1 << 20).unwrap(), 2);
        assert_eq!(std::fs::read(dest.join("yt-dlp.exe")).unwrap(), b"program");
        assert_eq!(std::fs::read(dest.join("_internal").join("lib.dll")).unwrap(), big);
        assert!(unpack(&z, &dest, 1000).is_err(), "too big");
        for bad in ["../evil.exe", "/etc/x", "C:/x", "a/../../x"] {
            assert!(unpack(&archive(&[(bad, b"x", false)]), &dest, 1 << 20).is_err(), "{bad}");
        }
        let mut broken = z.clone();
        broken[40] ^= 0xFF;
        assert!(unpack(&broken, &dest, 1 << 20).is_err(), "damaged data");
        let _ = std::fs::remove_dir_all(&dest);
    }
}
