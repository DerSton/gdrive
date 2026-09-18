use crate::config::AppConfig;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::io::{copy, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tracing::info;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const EMBEDDED_RCLONE_GZ: &[u8] = include_bytes!("../assets/rclone.gz");

pub fn engine_bin_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gdrive")
        .join("bin")
        .join("rclone")
}

pub fn ensure_engine() -> Result<PathBuf> {
    // 1. Check if rclone is available in PATH
    if let Ok(output) = Command::new("rclone").arg("version").output() {
        if output.status.success() {
            info!("Verwende System-rclone aus PATH");
            return Ok(PathBuf::from("rclone"));
        }
    }

    // 2. Check local data directory (~/.local/share/gdrive/bin/rclone)
    let local_bin = engine_bin_path();
    if local_bin.exists() {
        if let Ok(output) = Command::new(&local_bin).arg("version").output() {
            if output.status.success() {
                info!("Verwende vorhandene rclone-Engine: {:?}", local_bin);
                return Ok(local_bin);
            }
        }
    }

    // 3. Extract embedded rclone if available
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        info!("Entpacke eingebettete rclone-Engine nach {:?}", local_bin);
        if let Some(parent) = local_bin.parent() {
            fs::create_dir_all(parent)?;
        }

        let gz_reader = BufReader::new(EMBEDDED_RCLONE_GZ);
        let mut decoder = GzDecoder::new(gz_reader);
        let mut dest_file = File::create(&local_bin)?;
        copy(&mut decoder, &mut dest_file)?;

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&local_bin)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&local_bin, perms)?;
        }

        info!("rclone erfolgreich entpackt und betriebsbereit.");
        Ok(local_bin)
    }

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        anyhow::bail!("rclone-Engine nicht gefunden und keine eingebettete Version für diese Architektur verfügbar.")
    }
}

pub fn is_mounted(mount_point: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
            let path_str = mount_point.to_string_lossy();
            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[1] == path_str {
                    return true;
                }
            }
        }
    }
    false
}

pub fn unmount_drive(mount_point: &Path) -> Result<()> {
    if !mount_point.exists() {
        return Ok(());
    }

    info!("Trenne Google Drive Mount unter {:?}...", mount_point);
    #[cfg(target_os = "linux")]
    {
        // Try fusermount3 first
        let res3 = Command::new("fusermount3")
            .arg("-u")
            .arg("-z")
            .arg(mount_point)
            .output();

        if let Ok(out) = res3 {
            if out.status.success() {
                info!("Erfolgreich via fusermount3 getrennt.");
                return Ok(());
            }
        }

        // Fallback to fusermount
        let res = Command::new("fusermount")
            .arg("-u")
            .arg("-z")
            .arg(mount_point)
            .output();

        if let Ok(out) = res {
            if out.status.success() {
                info!("Erfolgreich via fusermount getrennt.");
                return Ok(());
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // For windows unmount (e.g. taskkill or WinFsp unmount)
    }

    Ok(())
}

pub fn mount_drive(config: &AppConfig) -> Result<Child> {
    let rclone_bin = ensure_engine()?;
    let rclone_conf = config.write_rclone_conf()?;
    let mount_dir = &config.mount_point;

    // Create mount directory if not present
    if !mount_dir.exists() {
        fs::create_dir_all(mount_dir).with_context(|| {
            format!("Konnte Mount-Verzeichnis {:?} nicht erstellen", mount_dir)
        })?;
    }

    // Clean up any stale mount
    let _ = unmount_drive(mount_dir);

    info!(
        "Starte rclone mount auf {:?} mit FUSE Cache-Modus 'full'...",
        mount_dir
    );

    let child = Command::new(rclone_bin)
        .arg("mount")
        .arg("gdrive:")
        .arg(mount_dir)
        .arg("--config")
        .arg(&rclone_conf)
        .arg("--vfs-cache-mode")
        .arg("full")
        .arg("--vfs-cache-max-age")
        .arg("24h")
        .arg("--vfs-read-chunk-size")
        .arg("32M")
        .arg("--vfs-read-chunk-size-limit")
        .arg("2G")
        .arg("--buffer-size")
        .arg("32M")
        .arg("--dir-cache-time")
        .arg("2m")
        .arg("--drive-export-formats")
        .arg("docx,xlsx,pptx,svg")
        .arg("--log-level")
        .arg("NOTICE")
        .arg("--rc")
        .arg("--rc-addr")
        .arg("127.0.0.1:5572")
        .arg("--rc-no-auth")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("Fehler beim Ausführen von rclone mount auf {:?}", mount_dir))?;

    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_bin_path() {
        let p = engine_bin_path();
        assert!(p.to_string_lossy().contains("gdrive"));
        assert!(p.to_string_lossy().ends_with("rclone"));
    }

    #[test]
    fn test_ensure_engine_extraction() {
        let bin = ensure_engine().expect("Engine should be resolved or extracted");
        assert!(bin.exists() || bin.to_string_lossy() == "rclone");
    }
}

