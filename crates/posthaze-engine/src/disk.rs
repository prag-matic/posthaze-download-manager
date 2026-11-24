//! Preflight disk-space and filesystem limit checks before creating `.part` files.

use std::path::Path;

use crate::EngineError;

/// Safety margin reserved above the download size when checking available disk space (128 MiB).
pub(crate) const DISK_MARGIN_BYTES: u64 = 128 * 1024 * 1024;

/// Maximum single-file size supported by FAT32 (`4 GiB - 1`).
pub(crate) const FAT32_MAX_BYTES: u64 = u32::MAX as u64;

/// Verifies that the target volume for `target_dir` has enough free space (plus safety margin)
/// and that its filesystem can store a file of `total_bytes`.
pub(crate) fn check_disk_preflight(target_dir: &Path, total_bytes: u64) -> Result<(), EngineError> {
    if total_bytes == 0 {
        return Ok(());
    }
    imp::check(target_dir, total_bytes)
}

/// Moves `path` to the OS Recycle Bin / Trash when supported, falling back to permanent deletion
/// on non-Windows platforms. Succeeds silently if `path` does not exist.
pub(crate) fn move_to_recycle_bin(path: &Path) -> Result<(), EngineError> {
    imp::recycle_bin(path)
}

fn validate_disk_limits(
    target_dir: &Path,
    total_bytes: u64,
    available_bytes: u64,
    filesystem_name: Option<&str>,
) -> Result<(), EngineError> {
    if let Some(fs) = filesystem_name
        && fs.eq_ignore_ascii_case("FAT32")
        && total_bytes > FAT32_MAX_BYTES
    {
        return Err(EngineError::FileTooLargeForFilesystem {
            path: target_dir.to_path_buf(),
            size_bytes: total_bytes,
            max_bytes: FAT32_MAX_BYTES,
        });
    }

    let required = total_bytes.saturating_add(DISK_MARGIN_BYTES);
    if available_bytes < required {
        return Err(EngineError::InsufficientDiskSpace {
            path: target_dir.to_path_buf(),
            required_bytes: required,
            available_bytes,
        });
    }

    Ok(())
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumePathNameW,
    };

    use super::validate_disk_limits;
    use crate::EngineError;

    fn existing_ancestor(path: &Path) -> PathBuf {
        let mut cur = Some(path);
        while let Some(p) = cur {
            if p.exists() {
                return p.to_path_buf();
            }
            cur = p.parent();
        }
        PathBuf::from(".")
    }

    fn to_wide_null(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn check(target_dir: &Path, total_bytes: u64) -> Result<(), EngineError> {
        let probe_dir = existing_ancestor(target_dir);
        let wide_dir = to_wide_null(probe_dir.as_os_str());

        let mut free_available: u64 = 0;
        // SAFETY: `wide_dir` is a valid null-terminated UTF-16 path and `free_available` is a
        // valid stack `u64`; the remaining optional output pointers are null.
        let space_ok = unsafe {
            GetDiskFreeSpaceExW(
                wide_dir.as_ptr(),
                &mut free_available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if space_ok == 0 {
            // Do not block the download if the OS query fails on an unusual UNC/virtual path.
            return Ok(());
        }

        let mut root_buf = vec![0u16; 260];
        let mut fs_name: Option<String> = None;
        // SAFETY: `wide_dir` is null-terminated and `root_buf` has capacity `root_buf.len()`.
        let got_root = unsafe {
            GetVolumePathNameW(wide_dir.as_ptr(), root_buf.as_mut_ptr(), root_buf.len() as u32)
        };
        if got_root != 0 {
            let mut fs_buf = [0u16; 64];
            // SAFETY: `root_buf` is null-terminated by `GetVolumePathNameW` and `fs_buf` is valid for 64 u16s.
            let got_vol = unsafe {
                GetVolumeInformationW(
                    root_buf.as_ptr(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    fs_buf.as_mut_ptr(),
                    fs_buf.len() as u32,
                )
            };
            if got_vol != 0 {
                let len = fs_buf.iter().position(|&c| c == 0).unwrap_or(fs_buf.len());
                fs_name = Some(String::from_utf16_lossy(&fs_buf[..len]));
            }
        }

        validate_disk_limits(target_dir, total_bytes, free_available, fs_name.as_deref())
    }

    pub fn recycle_bin(path: &Path) -> Result<(), EngineError> {
        use windows_sys::Win32::UI::Shell::{
            FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
            SHFILEOPSTRUCTW, SHFileOperationW,
        };

        if !path.exists() {
            return Ok(());
        }

        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
        };
        let normalized = abs.to_string_lossy();
        let clean = normalized.strip_prefix(r"\\?\").unwrap_or(&normalized);

        let mut wide: Vec<u16> = OsStr::new(clean).encode_wide().collect();
        wide.push(0);
        wide.push(0);

        let mut op = SHFILEOPSTRUCTW {
            hwnd: std::ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: wide.as_ptr(),
            pTo: std::ptr::null(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
            fAnyOperationsAborted: 0,
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: std::ptr::null(),
        };

        // SAFETY: `wide` is double-NUL-terminated UTF-16 and outlives `SHFileOperationW`;
        // `pTo` and `lpszProgressTitle` are null and unused for `FO_DELETE`.
        let rc = unsafe { SHFileOperationW(&mut op) };
        if rc != 0 && path.exists() {
            return Err(EngineError::Io(std::io::Error::other(format!(
                "Recycle Bin deletion failed (SHFileOperationW error {rc}) for {}",
                path.display()
            ))));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;

    use crate::EngineError;

    pub fn check(_target_dir: &Path, _total_bytes: u64) -> Result<(), EngineError> {
        Ok(())
    }

    pub fn recycle_bin(path: &Path) -> Result<(), EngineError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(EngineError::Io(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_disk_limits_rejects_insufficient_space_and_fat32_overflow() {
        let dir = Path::new("D:\\Downloads");

        let fat32_err = validate_disk_limits(
            dir,
            FAT32_MAX_BYTES + 1,
            100 * 1024 * 1024 * 1024,
            Some("FAT32"),
        )
        .unwrap_err();
        assert!(matches!(
            fat32_err,
            EngineError::FileTooLargeForFilesystem { .. }
        ));

        let ntfs_ok = validate_disk_limits(
            dir,
            FAT32_MAX_BYTES + 1,
            100 * 1024 * 1024 * 1024,
            Some("NTFS"),
        );
        assert!(ntfs_ok.is_ok());

        let space_err =
            validate_disk_limits(dir, 1024 * 1024, 1024 * 1024 + DISK_MARGIN_BYTES - 1, Some("NTFS"))
                .unwrap_err();
        assert!(matches!(
            space_err,
            EngineError::InsufficientDiskSpace { .. }
        ));

        let exact_ok =
            validate_disk_limits(dir, 1024 * 1024, 1024 * 1024 + DISK_MARGIN_BYTES, Some("NTFS"));
        assert!(exact_ok.is_ok());
    }
}
