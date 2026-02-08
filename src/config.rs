use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub profiles: Vec<ConnectionProfile>,
    #[serde(default)]
    pub active_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub name: String,
    pub address: String,
    pub namespace: String,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
    pub ca_path: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            tracing::info!("Config file not found at {:?}, using default config", config_path);
            return Ok(Self::default());
        }

        tracing::info!("Loading config from {:?}", config_path);
        let contents = std::fs::read_to_string(&config_path)?;
        let config: Self = serde_yaml::from_str(&contents)?;

        Ok(config)
    }

    fn get_config_path() -> Result<PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| anyhow::anyhow!("Could not determine home directory"))?;

        let mut path = PathBuf::from(home);
        path.push(".tuiporal");
        path.push("config.yaml");

        Ok(path)
    }

    pub fn get_active_profile(&self) -> Option<&ConnectionProfile> {
        if let Some(name) = &self.active_profile {
            self.profiles.iter().find(|p| &p.name == name)
        } else {
            self.profiles.first()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            profiles: vec![ConnectionProfile {
                name: "local".to_string(),
                address: "localhost:7233".to_string(),
                namespace: "default".to_string(),
                tls: None,
                api_key: None,
            }],
            active_profile: Some("local".to_string()),
        }
    }
}
