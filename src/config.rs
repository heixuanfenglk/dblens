use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::kind::BackendKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MssqlAuthMode {
    /// SQL Server Authentication (user + password).
    #[default]
    SqlServer,
    /// Windows / Integrated Authentication (current user or DOMAIN\user).
    Windows,
    EntraPassword,
    EntraIntegrated,
    EntraMfa,
    EntraManagedIdentity,
    EntraServicePrincipal,
}

impl MssqlAuthMode {
    pub const ALL: &'static [Self] = &[
        Self::SqlServer,
        Self::Windows,
        Self::EntraPassword,
        Self::EntraIntegrated,
        Self::EntraMfa,
        Self::EntraManagedIdentity,
        Self::EntraServicePrincipal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SqlServer => "SQL Server 验证",
            Self::Windows => "Windows 验证",
            Self::EntraPassword => "Microsoft Entra - Password",
            Self::EntraIntegrated => "Microsoft Entra - Integrated",
            Self::EntraMfa => "Microsoft Entra - MFA",
            Self::EntraManagedIdentity => "Microsoft Entra - Managed Identity",
            Self::EntraServicePrincipal => "Microsoft Entra - Service Principal",
        }
    }

    /// Whether username/password fields should be shown in the wizard.
    pub fn needs_credentials(self) -> bool {
        matches!(
            self,
            Self::SqlServer | Self::EntraPassword | Self::EntraServicePrincipal
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    #[serde(default = "new_id")]
    pub id: String,
    pub name: String,
    pub kind: BackendKind,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// SQL database / Redis DB index / SQLite 文件路径 / Mongo 默认库
    #[serde(default)]
    pub database: Option<String>,
    /// 完整 URL（若填写则优先于 host/port）
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub insecure: bool,
    /// MSSQL 验证方式（其它后端忽略）
    #[serde(default)]
    pub auth_mode: MssqlAuthMode,
    /// Snowflake warehouse（其它后端可选忽略）
    #[serde(default)]
    pub warehouse: Option<String>,
    /// Snowflake role（其它后端可选忽略）
    #[serde(default)]
    pub role: Option<String>,
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default: None,
            connections: Vec::new(),
        }
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "新连接".into(),
            kind: BackendKind::Mysql,
            host: "127.0.0.1".into(),
            port: 3306,
            username: None,
            password: None,
            database: None,
            url: None,
            insecure: false,
            auth_mode: MssqlAuthMode::default(),
            warehouse: None,
            role: None,
        }
    }
}

impl Connection {
    pub fn with_kind(kind: BackendKind) -> Self {
        let mut c = Self::default();
        c.kind = kind;
        c.name = format!("新{}", kind.display_name());
        c.port = kind.default_port();
        if kind == BackendKind::Sqlite {
            c.host.clear();
            c.database = Some("./data.db".into());
        }
        c
    }

    pub fn endpoint_label(&self) -> String {
        if let Some(url) = self.url.as_ref().filter(|u| !u.is_empty()) {
            return url.trim_end_matches('/').to_string();
        }
        match self.kind {
            BackendKind::Sqlite => self
                .database
                .clone()
                .unwrap_or_else(|| "(未指定文件)".into()),
            _ => {
                let host = self.host.trim();
                if host.contains("://") {
                    return host.trim_end_matches('/').to_string();
                }
                let port = if self.port == 0 {
                    self.kind.default_port()
                } else {
                    self.port
                };
                if port == 0 {
                    host.to_string()
                } else {
                    format!("{host}:{port}")
                }
            }
        }
    }

    /// Normalize pasted URLs so host/port/url fields stay consistent.
    pub fn normalize_endpoint(&mut self) {
        let raw = self
            .url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .or_else(|| {
                let h = self.host.trim();
                if h.contains("://") {
                    Some(h.to_string())
                } else {
                    None
                }
            });
        let Some(raw) = raw else {
            return;
        };
        let raw = raw.trim().trim_end_matches('/').to_string();
        if let Ok(u) = url::Url::parse(&raw) {
            self.url = Some(raw.clone());
            if let Some(host) = u.host_str() {
                self.host = host.to_string();
            }
            if let Some(port) = u.port() {
                self.port = port;
            } else if u.scheme() == "https" {
                self.port = 443;
            }
            // keep a friendly name if user pasted URL as name
            if self.name.contains("://") {
                self.name = if self.port == 0 {
                    self.host.clone()
                } else {
                    format!("{}:{}", self.host, self.port)
                };
            }
        } else {
            self.url = Some(raw);
        }
    }

    pub fn ensure_defaults(&mut self) {
        if self.id.trim().is_empty() {
            self.id = new_id();
        }
        self.normalize_endpoint();
        if self.port == 0 && self.kind != BackendKind::Sqlite {
            self.port = self.kind.default_port();
        }
        if self.host.trim().is_empty() && self.kind != BackendKind::Sqlite {
            self.host = "127.0.0.1".into();
        }
    }
}

impl Config {
    pub fn discover_path() -> PathBuf {
        let cwd = PathBuf::from("dblens.toml");
        if cwd.exists() {
            return absolutize(&cwd);
        }
        // migrate: still pick up old allink.toml in cwd if present
        let legacy = PathBuf::from("allink.toml");
        if legacy.exists() {
            return absolutize(&legacy);
        }
        let example = PathBuf::from("config.example.toml");
        if example.exists() && !cwd.exists() {
            // 仍返回目标路径，便于首次保存
        }
        let config_root = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        let preferred = config_root.join("dblens").join("config.toml");
        if preferred.exists() {
            return preferred;
        }
        let legacy_cfg = config_root.join("allink").join("config.toml");
        if legacy_cfg.exists() {
            return legacy_cfg;
        }
        preferred
    }

    pub fn load(explicit: Option<&Path>) -> Result<(Self, PathBuf)> {
        let path = match explicit {
            Some(p) => absolutize(p),
            None => {
                let app = crate::app_state::AppState::load();
                if let Some(last) = app.last_config_path.as_ref() {
                    let p = PathBuf::from(last);
                    if p.exists() {
                        p
                    } else {
                        Self::discover_path()
                    }
                } else {
                    Self::discover_path()
                }
            }
        };

        if !path.exists() {
            return Ok((Self::default(), path));
        }

        let text = fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text)
            .with_context(|| format!("解析配置失败: {}", path.display()))?;
        for c in &mut cfg.connections {
            c.ensure_defaults();
            if c.name.trim().is_empty() {
                bail!("连接 name 不能为空");
            }
        }
        Ok((cfg, path))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("序列化配置失败")?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, path).or_else(|_| fs::write(path, &text))?;
        Ok(())
    }

    pub fn find(&self, id_or_name: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.id == id_or_name || c.name == id_or_name)
    }

    pub fn find_mut(&mut self, id_or_name: &str) -> Option<&mut Connection> {
        self.connections
            .iter_mut()
            .find(|c| c.id == id_or_name || c.name == id_or_name)
    }
}

fn absolutize(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}
