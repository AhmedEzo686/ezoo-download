//! Passwords and cookies on disk: Windows DPAPI, or AES-256-GCM with an
//! app-specific random key stored in the macOS login Keychain. A copied JSON
//! file does not carry the key needed to decrypt its protected fields.
//!
//! Protected text starts with `dpapi:` or `keychain-aesgcm:v1:`. Files of older
//! versions hold plain text: it is read as it is and protected on the next
//! save. Use the field attributes below:
//!
//! ```ignore
//! #[serde(with = "crate::secret::string")]          pub password: String,
//! #[serde(default, with = "crate::secret::opt")]    pub cookies: Option<String>,
//! #[serde(default, with = "crate::secret::pair")]   pub auth: Option<(String, String)>,
//! ```

const PREFIX: &str = "dpapi:";
const MAC_PREFIX: &str = "keychain-aesgcm:v1:";

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

#[cfg(any(target_os = "macos", test))]
mod macos_crypto {
    use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
    use ring::rand::{SecureRandom, SystemRandom};

    pub fn protect(key: &[u8; 32], plain: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut nonce = [0u8; aead::NONCE_LEN];
        SystemRandom::new().fill(&mut nonce).map_err(|_| anyhow::anyhow!("could not generate a credential nonce"))?;
        let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| anyhow::anyhow!("invalid credential key"))?);
        let mut encrypted = plain.to_vec();
        key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(super::MAC_PREFIX), &mut encrypted)
            .map_err(|_| anyhow::anyhow!("could not encrypt credentials"))?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&encrypted);
        Ok(out)
    }

    pub fn unprotect(key: &[u8; 32], stored: &[u8]) -> Option<Vec<u8>> {
        if stored.len() < aead::NONCE_LEN + AES_256_GCM.tag_len() {
            return None;
        }
        let (nonce, encrypted) = stored.split_at(aead::NONCE_LEN);
        let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).ok()?);
        let mut plain = encrypted.to_vec();
        let opened = key.open_in_place(Nonce::try_assume_unique_for_key(nonce).ok()?, Aad::from(super::MAC_PREFIX), &mut plain).ok()?;
        Some(opened.to_vec())
    }
}

#[cfg(target_os = "macos")]
mod os {
    #[cfg(not(test))]
    fn key(create: bool) -> anyhow::Result<[u8; 32]> {
        use anyhow::Context;
        use ring::rand::{SecureRandom, SystemRandom};
        use security_framework::os::macos::keychain::SecKeychain;

        // Only this exact app-owned item is accessed. Never enumerate the
        // Keychain or replace an existing key, including a concurrent creator.
        const SERVICE: &str = "com.ezoo.EzooDownload.credentials";
        const ACCOUNT: &str = "settings-key-v1";
        const ITEM_NOT_FOUND: i32 = -25300;
        const DUPLICATE_ITEM: i32 = -25299;
        static KEY: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);
        let mut cached = KEY.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(key) = *cached {
            return Ok(key);
        }
        let keychain = SecKeychain::default().context("could not open the login Keychain to protect credentials")?;
        let read_key = || -> anyhow::Result<[u8; 32]> {
            let (password, _) = keychain.find_generic_password(SERVICE, ACCOUNT).context("could not read the Ezoo Download credential key")?;
            password.as_ref().try_into().map_err(|_| anyhow::anyhow!("the Ezoo Download credential key has an invalid size"))
        };
        let key = match keychain.find_generic_password(SERVICE, ACCOUNT) {
            Ok((password, _)) => password.as_ref().try_into().map_err(|_| anyhow::anyhow!("the Ezoo Download credential key has an invalid size"))?,
            Err(e) if e.code() == ITEM_NOT_FOUND && create => {
                let mut generated = [0u8; 32];
                SystemRandom::new().fill(&mut generated).map_err(|_| anyhow::anyhow!("could not generate a credential key"))?;
                match keychain.add_generic_password(SERVICE, ACCOUNT, &generated) {
                    Ok(()) => generated,
                    Err(e) if e.code() == DUPLICATE_ITEM => read_key()?,
                    Err(e) => return Err(e).context("could not save the Ezoo Download credential key; credentials were not saved"),
                }
            }
            Err(e) => return Err(e).context("could not read the Ezoo Download credential key"),
        };
        *cached = Some(key);
        Ok(key)
    }

    // Unit tests exercise encryption and migration with a synthetic key. They
    // must not read or modify the user's login Keychain.
    #[cfg(test)]
    std::thread_local! {
        static KEY_AVAILABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    }

    #[cfg(test)]
    fn key(_create: bool) -> anyhow::Result<[u8; 32]> {
        anyhow::ensure!(KEY_AVAILABLE.get(), "synthetic Keychain unavailable");
        Ok([0x42; 32])
    }

    #[cfg(test)]
    pub(super) fn with_unavailable_key<T>(run: impl FnOnce() -> T) -> T {
        struct Restore(bool);
        impl Drop for Restore {
            fn drop(&mut self) { KEY_AVAILABLE.set(self.0); }
        }
        let _restore = Restore(KEY_AVAILABLE.replace(false));
        run()
    }

    pub fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
        super::macos_crypto::protect(&key(true)?, data)
    }

    pub fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
        super::macos_crypto::unprotect(&key(false)?, data).ok_or_else(|| anyhow::anyhow!("saved credentials could not be authenticated"))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod os {
    // Platforms without a supported user key store keep the legacy format.
    pub fn protect(_data: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unprotect(_data: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

/// The text as it is kept on the disk.
pub fn seal(plain: &str) -> anyhow::Result<String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    #[cfg(target_os = "macos")]
    {
        seal_protected(plain, MAC_PREFIX, os::protect)
    }
    #[cfg(not(target_os = "macos"))]
    Ok(match os::protect(plain.as_bytes()) {
        Some(enc) => format!("{PREFIX}{}", to_hex(&enc)),
        None => plain.to_string(),
    })
}

#[cfg(any(target_os = "macos", test))]
fn seal_protected(plain: &str, prefix: &str, protect: impl FnOnce(&[u8]) -> anyhow::Result<Vec<u8>>) -> anyhow::Result<String> {
    // A Keychain/encryption failure propagates through serde; plaintext must
    // never silently become the fallback format on macOS.
    Ok(format!("{prefix}{}", to_hex(&protect(plain.as_bytes())?)))
}

/// macOS read failures propagate through serde, so a locked Keychain cannot
/// turn saved credentials into empty values that a later autosave overwrites.
/// Unreadable Windows DPAPI values retain the previous empty-string behavior.
pub fn open(stored: &str) -> anyhow::Result<String> {
    if let Some(hex) = stored.strip_prefix(MAC_PREFIX) {
        #[cfg(target_os = "macos")]
        {
            let encrypted = from_hex(hex).ok_or_else(|| anyhow::anyhow!("saved credentials have an invalid format"))?;
            return Ok(String::from_utf8(os::unprotect(&encrypted)?)?);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = hex;
            return Ok(String::new());
        }
    }
    Ok(match stored.strip_prefix(PREFIX) {
        #[cfg(not(target_os = "macos"))]
        Some(hex) => from_hex(hex)
            .and_then(|b| os::unprotect(&b))
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default(),
        #[cfg(target_os = "macos")]
        Some(_) => String::new(),
        None => stored.to_string(),
    })
}

#[cfg(all(target_os = "macos", test))]
pub(crate) fn with_unavailable_key<T>(run: impl FnOnce() -> T) -> T {
    os::with_unavailable_key(run)
}

pub mod string {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &str, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::seal(v).map_err(serde::ser::Error::custom)?)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        super::open(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

pub mod opt {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<String>, s: S) -> Result<S::Ok, S::Error> {
        v.as_deref().map(super::seal).transpose().map_err(serde::ser::Error::custom)?.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        Ok(Option::<String>::deserialize(d)?.map(|x| super::open(&x)).transpose().map_err(serde::de::Error::custom)?.filter(|x| !x.is_empty()))
    }
}

pub mod pair {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<(String, String)>, s: S) -> Result<S::Ok, S::Error> {
        v.as_ref().map(|(u, p)| super::seal(p).map(|p| (u.clone(), p))).transpose().map_err(serde::ser::Error::custom)?.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<(String, String)>, D::Error> {
        Option::<(String, String)>::deserialize(d)?.map(|(u, p)| super::open(&p).map(|p| (u, p))).transpose().map_err(serde::de::Error::custom)
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
        assert_eq!(open("dpapi:zz").unwrap(), "");
        assert_eq!(open("dpapi:00ff").unwrap(), ""); // not this user's key
        let t: T = serde_json::from_str(r#"{"p":"dpapi:1234","c":"dpapi:","a":null}"#).unwrap();
        assert_eq!(t.p, "");
        assert_eq!(t.c, None);
        assert_eq!(seal("").unwrap(), "");
    }

    #[test]
    fn authenticated_encryption_uses_fresh_nonces_and_rejects_tampering() {
        let key = [0x31; 32];
        let first = macos_crypto::protect(&key, b"synthetic-password").unwrap();
        let mut second = macos_crypto::protect(&key, b"synthetic-password").unwrap();
        assert_ne!(first, second);
        assert_eq!(macos_crypto::unprotect(&key, &first).unwrap(), b"synthetic-password");
        assert!(macos_crypto::unprotect(&[0x32; 32], &first).is_none());
        second[12] ^= 1;
        assert!(macos_crypto::unprotect(&key, &second).is_none());
        assert!(macos_crypto::unprotect(&key, &first[..12]).is_none());
    }

    #[test]
    fn failed_protection_never_returns_plaintext() {
        let result = seal_protected("synthetic-password", MAC_PREFIX, |_| anyhow::bail!("synthetic Keychain failure"));
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn legacy_plaintext_migrates_to_mac_encryption_on_save() {
        let value: T = serde_json::from_str(r#"{"p":"synthetic-password","c":"session=synthetic-cookie","a":["user","synthetic-auth"]}"#).unwrap();
        let saved = serde_json::to_string(&value).unwrap();
        assert!(saved.contains(MAC_PREFIX));
        for secret in ["synthetic-password", "synthetic-cookie", "synthetic-auth"] {
            assert!(!saved.contains(secret));
        }
        assert_eq!(serde_json::from_str::<T>(&saved).unwrap(), value);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unavailable_key_rejects_reads_and_saves_without_changing_existing_json() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("ezoo-keychain-failure-{}-{nanos:x}", std::process::id()));
        let path = dir.join("settings.json");
        let value = T { p: "synthetic-password".into(), c: Some("session=synthetic-cookie".into()), a: None };
        crate::store::write_json_atomic(&path, &value).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        with_unavailable_key(|| {
            assert!(serde_json::from_str::<T>(&original).is_err());
            assert!(crate::store::write_json_atomic(&path, &value).is_err());
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(serde_json::from_str::<T>(&original).unwrap(), value);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
