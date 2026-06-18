use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;

use crate::error::{SignerError, SignerResult};

const LOCK_FILENAME: &str = "daemon.lock";

pub struct DaemonInstanceLock {
    lock_file: File,
    path: PathBuf,
}

impl DaemonInstanceLock {
    pub fn acquire(state_dir: &Path, mode: &str) -> SignerResult<Self> {
        std::fs::create_dir_all(state_dir).map_err(|err| {
            SignerError::Other(format!(
                "failed to create daemon state dir {}: {err}",
                state_dir.display()
            ))
        })?;
        let path = state_dir.join(LOCK_FILENAME);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| {
                SignerError::Other(format!(
                    "failed to open daemon lock {}: {err}",
                    path.display()
                ))
            })?;
        if let Err(_err) = lock_exclusive_nonblocking(&file) {
            let mut existing = String::new();
            let _ = File::open(&path).and_then(|mut handle| handle.read_to_string(&mut existing));
            let detail = if existing.trim().is_empty() {
                String::new()
            } else {
                format!(" daemon_lock_metadata={}", existing.trim())
            };
            return Err(SignerError::DaemonAlreadyRunning {
                path: path.display().to_string(),
                detail,
            });
        }
        let payload = json!({
            "pid": std::process::id(),
            "mode": mode.trim(),
            "acquired_at": Utc::now().to_rfc3339(),
        });
        file.set_len(0).map_err(|err| {
            SignerError::Other(format!("failed to truncate daemon lock file: {err}"))
        })?;
        let mut handle = file.try_clone().map_err(|err| {
            SignerError::Other(format!("failed to clone daemon lock handle: {err}"))
        })?;
        handle
            .write_all(payload.to_string().as_bytes())
            .map_err(|err| {
                SignerError::Other(format!("failed to write daemon lock metadata: {err}"))
            })?;
        handle.flush().map_err(|err| {
            SignerError::Other(format!("failed to flush daemon lock metadata: {err}"))
        })?;
        Ok(Self {
            lock_file: file,
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DaemonInstanceLock {
    fn drop(&mut self) {
        unlock(&self.lock_file);
    }
}

#[cfg(unix)]
fn lock_exclusive_nonblocking(file: &File) -> Result<(), std::io::Error> {
    use std::os::unix::io::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn unlock(file: &File) {
    use std::os::unix::io::AsRawFd;
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

#[cfg(not(unix))]
fn lock_exclusive_nonblocking(_file: &File) -> Result<(), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "daemon instance lock requires unix",
    ))
}

#[cfg(not(unix))]
fn unlock(_file: &File) {}
