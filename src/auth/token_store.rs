use super::TokenSet;
use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn save(&self, token: &TokenSet) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(token)?;
        fs::write(&self.path, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&self.path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&self.path, perms)?;
        }
        Ok(())
    }

    pub fn load(&self) -> anyhow::Result<Option<TokenSet>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(&self.path)?;
        let token: TokenSet = serde_json::from_str(&json)?;
        Ok(Some(token))
    }

    #[allow(dead_code)]
    pub fn clear(&self) -> anyhow::Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}
