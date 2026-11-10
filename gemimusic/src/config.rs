use crate::api::models::AuthTokens;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub auth_tokens: Option<AuthTokens>,
    pub user_email: Option<String>,
    pub user_name: Option<String>,
    pub volume: f32,
    pub repeat_mode: bool,
    pub shuffle_mode: bool,
}

pub struct ConfigManager {
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Self {
        let base_dir = if cfg!(target_os = "android") {
            PathBuf::from("/data/data/com.gemimusic.app/files")
        } else {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("gemimusic")
        };

        let _ = fs::create_dir_all(&base_dir);
        let config_path = base_dir.join("config.json");

        Self { config_path }
    }

    pub fn load(&self) -> AppConfig {
        if let Ok(data) = fs::read_to_string(&self.config_path) {
            if let Ok(config) = serde_json::from_str(&data) {
                return config;
            }
        }
        AppConfig {
            volume: 0.8,
            ..Default::default()
        }
    }

    pub fn save(&self, config: &AppConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let json = serde_json::to_string_pretty(config)?;
        fs::write(&self.config_path, json)?;
        Ok(())
    }

    pub fn save_tokens(&self, tokens: AuthTokens, email: Option<String>, name: Option<String>) {
        let mut config = self.load();
        config.auth_tokens = Some(tokens);
        if let Some(e) = email {
            config.user_email = Some(e);
        }
        if let Some(n) = name {
            config.user_name = Some(n);
        }
        let _ = self.save(&config);
    }

    pub fn clear_auth(&self) {
        let mut config = self.load();
        config.auth_tokens = None;
        config.user_email = None;
        config.user_name = None;
        let _ = self.save(&config);
    }
}
