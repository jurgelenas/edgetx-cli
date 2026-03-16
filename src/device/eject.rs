use crate::error::RadioError;
use std::path::Path;

/// Safely eject the device at mount_point.
pub fn eject(mount_point: &Path) -> Result<(), RadioError> {
    log::info!("ejecting device...");

    #[cfg(target_os = "linux")]
    {
        eject_linux(mount_point)
    }

    #[cfg(target_os = "macos")]
    {
        eject_macos(mount_point)
    }

    #[cfg(target_os = "windows")]
    {
        eject_windows(mount_point)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(RadioError::Eject("unsupported platform for eject".into()))
    }
}

#[cfg(target_os = "linux")]
fn eject_linux(mount_point: &Path) -> Result<(), RadioError> {
    use std::process::Command;

    let mount_str = mount_point.display().to_string();

    // Find the block device
    let output = Command::new("findmnt")
        .args(["-no", "SOURCE", &mount_str])
        .output()
        .map_err(|e| RadioError::Eject(format!("could not determine block device: {e}")))?;

    let block_device = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if block_device.is_empty() {
        return Err(RadioError::Eject(format!(
            "could not determine block device for {mount_str}"
        )));
    }

    let disk = strip_partition_number(&block_device);

    // Sync
    let _ = Command::new("sync").status();

    // Unmount
    let output = Command::new("udisksctl")
        .args(["unmount", "-b", &block_device, "--no-user-interaction"])
        .output()
        .map_err(|e| RadioError::Eject(format!("unmount failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(RadioError::Eject(format!("unmount failed: {stderr}")));
    }

    // Power off
    let output = Command::new("udisksctl")
        .args(["power-off", "-b", &disk, "--no-user-interaction"])
        .output()
        .map_err(|e| RadioError::Eject(format!("power-off failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(RadioError::Eject(format!("power-off failed: {stderr}")));
    }

    log::info!("ejected {} ({})", block_device, disk);
    Ok(())
}

#[cfg(target_os = "linux")]
fn strip_partition_number(device: &str) -> String {
    let mut d = device.to_string();
    while d.ends_with(|c: char| c.is_ascii_digit()) {
        d.pop();
    }
    d
}

#[cfg(target_os = "macos")]
fn eject_macos(mount_point: &Path) -> Result<(), RadioError> {
    use std::process::Command;

    let mount_str = mount_point.display().to_string();

    let output = Command::new("diskutil")
        .args(["unmount", &mount_str])
        .output()
        .map_err(|e| RadioError::Eject(format!("unmount failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(RadioError::Eject(format!("unmount failed: {stderr}")));
    }

    let output = Command::new("diskutil")
        .args(["eject", &mount_str])
        .output()
        .map_err(|e| RadioError::Eject(format!("eject failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(RadioError::Eject(format!("eject failed: {stderr}")));
    }

    log::info!("ejected {}", mount_str);
    Ok(())
}

#[cfg(target_os = "windows")]
fn eject_windows(mount_point: &Path) -> Result<(), RadioError> {
    use std::process::Command;

    let mount_str = mount_point.display().to_string();
    let drive_letter = if mount_str.len() >= 2 && mount_str.as_bytes()[1] == b':' {
        format!("{}:\\", &mount_str[..1])
    } else {
        mount_str.clone()
    };

    let output = Command::new("mountvol")
        .args([&drive_letter, "/D"])
        .output()
        .map_err(|e| RadioError::Eject(format!("eject failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(RadioError::Eject(format!("eject failed: {stderr}")));
    }

    log::info!("ejected {}", drive_letter);
    Ok(())
}
