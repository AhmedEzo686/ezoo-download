//! Positional writes so many connections can write into one file at once.

use std::fs::File;
use std::io;

#[allow(unused_mut)]
pub fn write_at(file: &File, buf: &[u8], mut offset: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut rest = buf;
        while !rest.is_empty() {
            let n = file.seek_write(rest, offset)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "failed to write data"));
            }
            rest = &rest[n..];
            offset += n as u64;
        }
        Ok(())
    }
}

/// On NTFS, writing far into a freshly sized file forces Windows to zero-fill
/// everything before it, which stalls big downloads. Marking the file sparse
/// avoids that. Best effort: failures are ignored.
pub fn make_sparse(file: &File) {
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
        use windows_sys::Win32::System::IO::DeviceIoControl;
        let mut returned: u32 = 0;
        DeviceIoControl(
            file.as_raw_handle() as _,
            FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        );
    }
    #[cfg(not(windows))]
    let _ = file;
}
