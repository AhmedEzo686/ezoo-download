//! Passwords and cookies on the disk: encrypted with the Windows user's own
//! key (DPAPI, `CryptProtectData`). Only the same Windows user on the same
//! computer can read them back; a settings or list file copied elsewhere (or
//! read by another user) shows no password.
//!
//! On the disk a protected text looks like `"dpapi:<hex>"`. Files of older
//! versions hold plain text: it is read as it is and protected on the next
//! save. Use the field attributes below:
//!
//! ```ignore
//! #[serde(with = "crate::secret::string")]          pub password: String,
//! #[serde(default, with = "crate::secret::opt")]    pub cookies: Option<String>,
//! #[serde(default, with = "crate::secret::pair")]   pub auth: Option<(String, String)>,
//! ```

const PREFIX: &str = "dpapi:";

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(windows)]
mod os {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn run(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: u32::try_from(data.len()).ok()?, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: `input` points at `data`, which outlives the call and is only
        // read; on success Windows allocates `out` (LocalAlloc), which is
        // copied and then freed with LocalFree exactly once.
        unsafe {
            let ok = if protect {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            };
            if ok == 0 || out.pbData.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(out.pbData as _);
            Some(bytes)
        }
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, true)
    }
    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, false)
    }
}

#[cfg(not(windows))]
mod os {
    // other systems (the tests): no user key store, so nothing is encrypted
    pub fn protect(_data: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unprotect(_data: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

/// The text as it is kept on the disk.
pub fn seal(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    match os::protect(plain.as_bytes()) {
        Some(enc) => format!("{PREFIX}{}", to_hex(&enc)),
        None => plain.to_string(),
    }
}

/// The text from the disk. A protected text that can't be opened (another
/// user or computer) becomes empty: the user types the password again.
pub fn open(stored: &str) -> String {
    match stored.strip_prefix(PREFIX) {
        Some(hex) => from_hex(hex)
            .and_then(|b| os::unprotect(&b))
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default(),
        None => stored.to_string(),
    }
}

pub mod string {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &str, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::seal(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        Ok(super::open(&String::deserialize(d)?))
    }
}

pub mod opt {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<String>, s: S) -> Result<S::Ok, S::Error> {
        v.as_deref().map(super::seal).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        Ok(Option::<String>::deserialize(d)?.map(|x| super::open(&x)).filter(|x| !x.is_empty()))
    }
}

pub mod pair {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<(String, String)>, s: S) -> Result<S::Ok, S::Error> {
        v.as_ref().map(|(u, p)| (u.clone(), super::seal(p))).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<(String, String)>, D::Error> {
        Ok(Option::<(String, String)>::deserialize(d)?.map(|(u, p)| (u, super::open(&p))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct T {
        #[serde(with = "string")]
        p: String,
        #[serde(default, with = "opt")]
        c: Option<String>,
        #[serde(default, with = "pair")]
        a: Option<(String, String)>,
    }

    #[test]
    fn plain_text_of_old_files_still_reads() {
        let t: T = serde_json::from_str(r#"{"p":"pw","c":"k=v","a":["u","x"]}"#).unwrap();
        assert_eq!(t, T { p: "pw".into(), c: Some("k=v".into()), a: Some(("u".into(), "x".into())) });
        let back: T = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn unreadable_protected_text_is_empty_not_garbage() {
        assert_eq!(open("dpapi:zz"), "");
        assert_eq!(open("dpapi:00ff"), ""); // not this user's key
        let t: T = serde_json::from_str(r#"{"p":"dpapi:1234","c":"dpapi:","a":null}"#).unwrap();
        assert_eq!(t.p, "");
        assert_eq!(t.c, None);
        assert_eq!(seal(""), "");
    }
}
