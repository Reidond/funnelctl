use std::path::{Path, PathBuf};

use crate::error::{FunnelError, Result};

pub fn runtime_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(path).join("funnelctl");
        ensure_dir(&path)?;
        return Ok(path);
    }

    let path = state_dir()?;
    ensure_dir(&path)?;
    Ok(path)
}

pub fn state_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        let path = PathBuf::from(path).join("funnelctl");
        ensure_dir(&path)?;
        return Ok(path);
    }

    if cfg!(target_os = "macos") {
        let path = home_dir()?.join("Library/Application Support/funnelctl");
        ensure_dir(&path)?;
        return Ok(path);
    }

    let path = home_dir()?.join(".local/state/funnelctl");
    ensure_dir(&path)?;
    Ok(path)
}

pub fn config_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(path).join("funnelctl");
        ensure_dir(&path)?;
        return Ok(path);
    }

    if cfg!(target_os = "macos") {
        let path = home_dir()?.join("Library/Application Support/funnelctl");
        ensure_dir(&path)?;
        return Ok(path);
    }

    let path = home_dir()?.join(".config/funnelctl");
    ensure_dir(&path)?;
    Ok(path)
}

pub fn cache_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        let path = PathBuf::from(path).join("funnelctl");
        ensure_dir(&path)?;
        return Ok(path);
    }

    if cfg!(target_os = "macos") {
        let path = home_dir()?.join("Library/Caches/funnelctl");
        ensure_dir(&path)?;
        return Ok(path);
    }

    let path = home_dir()?.join(".cache/funnelctl");
    ensure_dir(&path)?;
    Ok(path)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| FunnelError::Other("Unable to resolve HOME directory".to_string()))
}

fn ensure_dir(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(path).map_err(|err| {
        FunnelError::Other(format!("Failed to create {}: {}", path.display(), err))
    })?;
    set_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o700);
    std::fs::set_permissions(path, permissions).map_err(|err| {
        FunnelError::Other(format!(
            "Failed to set permissions on {}: {}",
            path.display(),
            err
        ))
    })
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
