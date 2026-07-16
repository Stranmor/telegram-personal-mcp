use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub(crate) fn restrict_private_directory(path: &Path) -> std::io::Result<()> {
    set_mode(path, 0o700)
}

pub(crate) fn restrict_private_file(path: &Path) -> std::io::Result<()> {
    set_mode(path, 0o600)
}

pub(crate) fn restrict_immutable_file(path: &Path) -> std::io::Result<()> {
    set_mode(path, 0o400)
}

#[cfg(unix)]
pub(crate) fn directory_is_owner_only(path: &Path) -> std::io::Result<bool> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o077 == 0)
}

#[cfg(not(unix))]
pub(crate) fn directory_is_owner_only(_path: &Path) -> std::io::Result<bool> {
    Ok(true)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}
