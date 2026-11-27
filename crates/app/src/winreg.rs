//! The user's own part of the registry (HKEY_CURRENT_USER), read and written
//! through Windows' own registry functions. (Earlier versions ran reg.exe
//! hidden for this; antivirus programs take a program that starts system
//! tools out of sight for one that hides what it does.)
//!
//! Keys are written as "HKCU\Software\..."; `name` None is the key's default
//! value. Only text values (REG_SZ) are used.

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_EXPAND_SZ,
        RRF_RT_REG_SZ,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// "HKCU\Software\X" -> "Software\X" (only the user's own part is used).
    fn sub(key: &str) -> Option<Vec<u16>> {
        let rest = key.strip_prefix(r"HKCU\").or_else(|| key.strip_prefix(r"HKEY_CURRENT_USER\"))?;
        Some(wide(rest))
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: an open key of ours, closed once
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open(key: &str, access: u32) -> Option<Key> {
        let path = sub(key)?;
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: `path` is NUL-terminated and lives through the call
        let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut h) };
        (r == ERROR_SUCCESS).then_some(Key(h))
    }

    pub fn get(key: &str, name: Option<&str>) -> Option<String> {
        let path = sub(key)?;
        let value = name.map(wide);
        let vp = value.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        let mut size: u32 = 0;
        // SAFETY: first call only asks for the size
        let r = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), vp, flags, std::ptr::null_mut(), std::ptr::null_mut(), &mut size)
        };
        if r != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: `buf` holds `size` bytes
        let r = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), vp, flags, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size)
        };
        if r != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    pub fn set(key: &str, name: Option<&str>, data: &str) -> bool {
        let Some(path) = sub(key) else { return false };
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: `path` is NUL-terminated; the key is closed by `Key`
        let r = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut h,
                std::ptr::null_mut(),
            )
        };
        if r != ERROR_SUCCESS {
            return false;
        }
        let k = Key(h);
        let value = name.map(wide);
        let vp = value.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
        let d = wide(data);
        // SAFETY: `d` holds its bytes with the closing NUL
        let r = unsafe { RegSetValueExW(k.0, vp, 0, REG_SZ, d.as_ptr().cast(), (d.len() * 2) as u32) };
        r == ERROR_SUCCESS
    }

    /// The value gone (true also when it was not there).
    pub fn delete_value(key: &str, name: Option<&str>) -> bool {
        let Some(k) = open(key, KEY_SET_VALUE) else { return true };
        let value = name.map(wide);
        let vp = value.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
        // SAFETY: an open key and a NUL-terminated name (or none)
        let r = unsafe { RegDeleteValueW(k.0, vp) };
        r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND
    }

    /// The key with everything under it gone (true also when it was not there).
    pub fn delete_key(key: &str) -> bool {
        let Some(path) = sub(key) else { return false };
        // SAFETY: `path` is NUL-terminated
        let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr()) };
        r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND
    }

    pub fn exists(key: &str, name: Option<&str>) -> bool {
        if name.is_none() {
            return open(key, KEY_READ).is_some();
        }
        get(key, name).is_some()
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
mod imp {
    pub fn get(_key: &str, _name: Option<&str>) -> Option<String> {
        None
    }
    pub fn set(_key: &str, _name: Option<&str>, _data: &str) -> bool {
        false
    }
    pub fn delete_value(_key: &str, _name: Option<&str>) -> bool {
        true
    }
    pub fn delete_key(_key: &str) -> bool {
        true
    }
    pub fn exists(_key: &str, _name: Option<&str>) -> bool {
        false
    }
}

#[allow(unused_imports)]
pub(crate) use imp::{delete_key, delete_value, exists, get, set};
