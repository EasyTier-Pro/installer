pub mod device_flow;
pub mod token_store;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_in: u64,
    #[serde(default = "Utc::now")]
    pub obtained_at: DateTime<Utc>,
}

impl TokenSet {
    pub fn is_expired(&self) -> bool {
        let expires_at = self.obtained_at + chrono::Duration::seconds(self.expires_in as i64);
        Utc::now() >= expires_at - chrono::Duration::minutes(5)
    }
}

#[derive(Debug, Clone)]
pub struct DeviceAuthInfo {
    pub device_code: String,
    #[allow(dead_code)]
    pub user_code: String,
    pub verification_uri: String,
    #[allow(dead_code)]
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(rename = "verification_uri_complete", default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TokenExchangeResponse {
    pub access_token: String,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_in: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OAuthError {
    pub error: String,
    #[serde(rename = "error_description", default)]
    pub description: Option<String>,
    #[serde(default)]
    pub interval: Option<u64>,
}
