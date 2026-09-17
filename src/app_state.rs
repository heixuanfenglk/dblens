use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    pub last_config_path: Option<String>,
}

impl AppState {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dblens")
            .join("app_state.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("序列化 app_state 失败")?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, &path).or_else(|_| fs::write(&path, &text))?;
        Ok(())
    }

    pub fn remember_config(&mut self, config_path: &Path) {
        self.last_config_path = Some(config_path.to_string_lossy().to_string());
        let _ = self.save();
    }
}
