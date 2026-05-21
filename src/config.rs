use directories::ProjectDirs;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct ConfigFile {
    server: Option<String>,
}

pub struct Config {
    pub console_base_url: String,
    pub credentials_path: PathBuf,
}

impl Config {
    pub fn new(server_flag: Option<String>) -> anyhow::Result<Self> {
        let base_url = if let Some(flag) = server_flag {
            flag
        } else if let Ok(env) = std::env::var("EASYTIER_CONSOLE_URL") {
            env
        } else {
            Self::read_config_file()
                .unwrap_or_else(|| "https://api.console.easytier.net".to_string())
        };

        let dirs = ProjectDirs::from("cn", "easytier", "console")
            .ok_or_else(|| anyhow::anyhow!("无法确定配置目录"))?;

        let creds = dirs.config_dir().join("credentials.json");
        std::fs::create_dir_all(dirs.config_dir())?;

        Ok(Self {
            console_base_url: base_url,
            credentials_path: creds,
        })
    }

    fn read_config_file() -> Option<String> {
        let dirs = ProjectDirs::from("cn", "easytier", "console")?;
        let config_path = dirs.config_dir().join("config.toml");
        let content = std::fs::read_to_string(config_path).ok()?;
        toml::from_str::<ConfigFile>(&content).ok()?.server
    }
}
