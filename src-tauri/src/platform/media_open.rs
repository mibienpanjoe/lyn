use std::{path::Path, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaOpenError {
    Unavailable,
}

pub(crate) trait MediaOpenPlatform {
    fn open(&self, path: &Path) -> Result<(), MediaOpenError>;
}

#[derive(Default)]
pub(crate) struct NativeMediaOpenPlatform;

impl MediaOpenPlatform for NativeMediaOpenPlatform {
    fn open(&self, path: &Path) -> Result<(), MediaOpenError> {
        let mut command = platform_command(path)?;
        command
            .spawn()
            .map(|_| ())
            .map_err(|_| MediaOpenError::Unavailable)
    }
}

#[cfg(target_os = "linux")]
fn platform_command(path: &Path) -> Result<Command, MediaOpenError> {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    Ok(command)
}

#[cfg(target_os = "macos")]
fn platform_command(path: &Path) -> Result<Command, MediaOpenError> {
    let mut command = Command::new("open");
    command.arg(path);
    Ok(command)
}

#[cfg(windows)]
fn platform_command(path: &Path) -> Result<Command, MediaOpenError> {
    let mut command = Command::new("explorer");
    command.arg(path);
    Ok(command)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn platform_command(_path: &Path) -> Result<Command, MediaOpenError> {
    Err(MediaOpenError::Unavailable)
}
