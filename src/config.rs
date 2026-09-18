use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub client_id: String,
    pub client_secret: String,
    pub mount_point: PathBuf,
    pub autostart: bool,
    pub token_json: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            mount_point: home.join("GoogleDrive"),
            autostart: true,
            token_json: None,
        }
    }
}

impl AppConfig {
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("gdrive")
    }

    pub fn config_file() -> PathBuf {
        Self::config_dir().join("config.json")
    }

    pub fn rclone_conf_file() -> PathBuf {
        Self::config_dir().join("rclone.conf")
    }

    pub fn is_configured(&self) -> bool {
        !self.client_id.trim().is_empty()
            && !self.client_secret.trim().is_empty()
            && self.token_json.is_some()
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::config_file();
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Konnte Konfiguration nicht lesen: {:?}", path))?;
        let config: AppConfig = serde_json::from_str(&content)
            .with_context(|| format!("Ungültiges Konfigurationsformat in {:?}", path))?;
        Ok(Some(config))
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir();
        fs::create_dir_all(&dir)
            .with_context(|| format!("Konnte Konfigurationsverzeichnis nicht erstellen: {:?}", dir))?;

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&dir)?.permissions();
            perms.set_mode(0o700);
            fs::set_permissions(&dir, perms)?;
        }

        let file = Self::config_file();
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&file, json)
            .with_context(|| format!("Konnte Konfiguration nicht schreiben: {:?}", file))?;

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&file)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&file, perms)?;
        }

        self.write_rclone_conf()?;
        Ok(())
    }

    pub fn write_rclone_conf(&self) -> Result<PathBuf> {
        let dir = Self::config_dir();
        fs::create_dir_all(&dir)?;

        let rclone_conf = Self::rclone_conf_file();
        let token_str = self.token_json.as_deref().unwrap_or("");

        let content = format!(
            "[gdrive]\ntype = drive\nclient_id = {}\nclient_secret = {}\nscope = drive\ntoken = {}\n",
            self.client_id.trim(),
            self.client_secret.trim(),
            token_str
        );

        fs::write(&rclone_conf, content)?;

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&rclone_conf)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&rclone_conf, perms)?;
        }

        Ok(rclone_conf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_configured() {
        let mut cfg = AppConfig::default();
        assert!(!cfg.is_configured());

        cfg.client_id = "test-id".into();
        cfg.client_secret = "test-secret".into();
        assert!(!cfg.is_configured()); // Still missing token

        cfg.token_json = Some("{\"access_token\":\"ya29...\"}".into());
        assert!(cfg.is_configured());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.client_id = "my-id".into();
        cfg.client_secret = "my-secret".into();
        cfg.token_json = Some("{\"access_token\":\"test\"}".into());

        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.client_id, "my-id");
        assert_eq!(deserialized.client_secret, "my-secret");
        assert_eq!(deserialized.token_json, Some("{\"access_token\":\"test\"}".into()));
    }
}
