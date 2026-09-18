use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::info;

pub fn autostart_desktop_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart")
        .join("gdrive.desktop")
}

pub fn is_autostart_enabled() -> bool {
    autostart_desktop_path().exists()
}

pub fn enable_autostart() -> Result<()> {
    let path = autostart_desktop_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let current_exe = std::env::current_exe().context("Konnte Executable-Pfad nicht ermitteln")?;
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Google Drive\n\
         Comment=Google Drive Tray Microservice\n\
         Exec={}\n\
         Icon=folder-gdrive\n\
         Terminal=false\n\
         Categories=Network;FileTransfer;\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n",
        current_exe.to_string_lossy()
    );

    fs::write(&path, content)
        .with_context(|| format!("Konnte Autostart-Datei nicht schreiben: {:?}", path))?;
    info!("Autostart erfolgreich aktiviert: {:?}", path);
    Ok(())
}

pub fn disable_autostart() -> Result<()> {
    let path = autostart_desktop_path();
    if path.exists() {
        fs::remove_file(&path)?;
        info!("Autostart deaktiviert: {:?}", path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_path() {
        let path = autostart_desktop_path();
        assert!(path.to_string_lossy().contains("autostart"));
        assert!(path.to_string_lossy().ends_with("gdrive.desktop"));
    }
}

