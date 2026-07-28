use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub service: ServiceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub pin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    #[serde(default = "default_service_name")]
    pub name: String,
}

fn default_bind() -> String {
    "0.0.0.0".into()
}

fn default_port() -> u16 {
    9527
}

fn default_service_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "ClipLink".into())
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            port: default_port(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            pin: String::new(),
        }
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            name: default_service_name(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            auth: AuthConfig::default(),
            service: ServiceConfig::default(),
        }
    }
}

impl Config {
    /// Load config from the first found path:
    ///   1. ./cliplinkd.toml
    ///   2. ~/.config/cliplinkd/cliplinkd.toml
    /// Falls back to defaults if none found.
    pub fn load() -> anyhow::Result<Self> {
        let paths: Vec<PathBuf> = vec![
            PathBuf::from("cliplinkd.toml"),
            dirs::config_dir()
                .unwrap_or_default()
                .join("cliplinkd")
                .join("cliplinkd.toml"),
        ];

        for path in &paths {
            if path.exists() {
                let content = std::fs::read_to_string(path).with_context(|| {
                    format!("Failed to read config from {}", path.display())
                })?;
                let config: Config = toml::from_str(&content).with_context(|| {
                    format!("Failed to parse config from {}", path.display())
                })?;
                tracing::info!("Loaded config from {}", path.display());
                return Ok(config);
            }
        }

        tracing::info!("No config file found, using defaults");
        Ok(Config::default())
    }
}
