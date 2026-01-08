use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use fs4::FileExt;

use crate::dirs;
use crate::error::{FunnelError, Result};

pub struct LockGuard {
    _file: File,
}

impl LockGuard {
    pub fn acquire() -> Result<Self> {
        let path = lock_path()?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|err| {
                FunnelError::Other(format!(
                    "Failed to open lock file {}: {}",
                    path.display(),
                    err
                ))
            })?;

        if file.try_lock_exclusive().is_ok() {
            write_pid(&mut file)?;
            return Ok(Self { _file: file });
        }

        let pid = read_pid(&mut file).ok();
        if let Some(pid) = pid {
            if !pid_is_alive(pid) && file.try_lock_exclusive().is_ok() {
                write_pid(&mut file)?;
                return Ok(Self { _file: file });
            }
            return Err(FunnelError::Conflict {
                source: None,
                context: format!("Another funnelctl instance is running (PID {})", pid),
            });
        }

        Err(FunnelError::Conflict {
            source: None,
            context: "Another funnelctl instance is running".to_string(),
        })
    }
}

fn lock_path() -> Result<PathBuf> {
    let dir = dirs::runtime_dir()?;
    Ok(dir.join("funnelctl.lock"))
}

fn write_pid(file: &mut File) -> Result<()> {
    let pid = std::process::id();
    file.set_len(0)
        .map_err(|err| FunnelError::Other(format!("Failed to truncate lock file: {}", err)))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|err| FunnelError::Other(format!("Failed to seek lock file: {}", err)))?;
    write!(file, "{}", pid)
        .map_err(|err| FunnelError::Other(format!("Failed to write lock file: {}", err)))?;
    file.flush()
        .map_err(|err| FunnelError::Other(format!("Failed to flush lock file: {}", err)))?;
    Ok(())
}

fn read_pid(file: &mut File) -> Result<u32> {
    let mut contents = String::new();
    file.seek(SeekFrom::Start(0))
        .map_err(|err| FunnelError::Other(format!("Failed to seek lock file: {}", err)))?;
    file.read_to_string(&mut contents)
        .map_err(|err| FunnelError::Other(format!("Failed to read lock file: {}", err)))?;
    let pid = contents
        .trim()
        .parse::<u32>()
        .map_err(|err| FunnelError::Other(format!("Failed to parse lock PID: {}", err)))?;
    Ok(pid)
}

fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    unsafe {
        let result = libc::kill(pid as i32, 0);
        if result == 0 {
            return true;
        }
        let err = std::io::Error::last_os_error();
        matches!(err.raw_os_error(), Some(libc::EPERM))
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}
