use std::fs;
use std::io;
use std::path::Path;

#[cfg(unix)]
use std::os::fd::AsRawFd;

/// A short-lived cross-process lock for pack publication and pack-family edits.
///
/// Unix keeps the directory descriptor open while `flock` owns the lock. Windows
/// uses a named kernel mutex derived from the canonical directory path. Both
/// ownership mechanisms are released by the operating system if the process
/// exits without running `Drop`.
pub(crate) struct PackOperationLock {
    #[cfg(unix)]
    file: fs::File,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(not(any(unix, windows)))]
    unsupported: (),
}

impl PackOperationLock {
    pub(crate) fn acquire(pack_dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(pack_dir)?;

        #[cfg(unix)]
        {
            let file = fs::File::open(pack_dir)?;
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
            return Ok(Self { file });
        }

        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use std::ptr;
            use windows_sys::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0};
            use windows_sys::Win32::System::Threading::{
                CreateMutexW, INFINITE, WaitForSingleObject,
            };

            let canonical = fs::canonicalize(pack_dir)?;
            let mut hash = 0xcbf29ce484222325_u64;
            for unit in canonical.as_os_str().encode_wide() {
                for byte in unit.to_le_bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x100000001b3);
                }
            }
            let name = format!("Global\\zmin-pack-operation-{hash:016x}");
            let name = name
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
            if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                unsafe {
                    CloseHandle(handle);
                }
                return Err(io::Error::last_os_error());
            }
            return Ok(Self { handle });
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = pack_dir;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "pack operation locking is unsupported on this platform",
            ))
        }
    }
}

impl Drop for PackOperationLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }

        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            let _ = ReleaseMutex(self.handle);
            let _ = CloseHandle(self.handle);
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = self.unsupported;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn serializes_threads_and_releases_after_panic() {
        let temp = tempfile::tempdir().expect("pack lock tempdir");
        let pack_dir = temp.path().join("pack");
        let first = PackOperationLock::acquire(&pack_dir).expect("first lock");
        let (started_tx, started_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let worker_pack_dir = pack_dir.clone();
        let worker = thread::spawn(move || {
            started_tx.send(()).expect("worker started");
            let _lock = PackOperationLock::acquire(&worker_pack_dir).expect("second lock");
            acquired_tx.send(()).expect("worker acquired");
        });
        started_rx.recv().expect("worker start signal");
        assert!(acquired_rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(first);
        acquired_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker proceeds after release");
        worker.join().expect("worker join");

        let panic_result = std::panic::catch_unwind(|| {
            let _lock = PackOperationLock::acquire(&pack_dir).expect("panic lock");
            panic!("release lock during unwind");
        });
        assert!(panic_result.is_err());
        let _after_panic = PackOperationLock::acquire(&pack_dir).expect("lock after panic");
    }
}
