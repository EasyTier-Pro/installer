use directories::ProjectDirs;
use std::path::PathBuf;

pub struct Config {
    pub console_base_url: String,
    pub credentials_path: PathBuf,
}

impl Config {
    pub fn new(server_flag: Option<String>) -> anyhow::Result<Self> {
        // Priority: CLI flag > env var > config file > default
        let base_url = if let Some(flag) = server_flag {
            flag
        } else if let Ok(env) = std::env::var("EASYTIER_CONSOLE_URL") {
            env
        } else {
            // Try reading from config file
            Self::read_config_file().unwrap_or_else(|| "https://console.easytier.cn".to_string())
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
        if !config_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(config_path).ok()?;
        content
            .lines()
            .find(|line| line.trim().starts_with("server"))
            .and_then(|line| line.splitn(2, '=').nth(1))
            .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
    }
}
