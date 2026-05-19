use crate::auth::token_store::TokenStore;
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MeResponse {
    #[allow(dead_code)]
    pub user: UserSummary,
    pub tenants: Vec<TenantSummary>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct UserSummary {
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub email: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TenantSummary {
    pub id: String,
    pub name: String,
    #[allow(dead_code)]
    pub role: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DeviceEnrollmentKey {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub key_code: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub tags: Vec<String>,
    #[allow(dead_code)]
    pub reusable: bool,
    #[allow(dead_code)]
    pub pre_approved: bool,
    #[serde(default)]
    #[allow(dead_code)]
    pub expires_at: Option<String>,
    pub revoked: bool,
    pub lifecycle_state: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateDeviceEnrollmentKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    pub reusable: bool,
    pub pre_approved: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CreateDeviceEnrollmentKeyResponse {
    pub enrollment_key: DeviceEnrollmentKey,
    pub bootstrap_token: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DeviceEnrollmentKeySecret {
    pub bootstrap_token: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct GetStartedResponse {
    #[serde(default)]
    pub config_server_url: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub commands: std::collections::HashMap<String, String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub downloads: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub release_channels: GetStartedReleaseChannels,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct GetStartedReleaseChannels {
    #[serde(default)]
    pub stable: GetStartedReleaseChannel,
    #[serde(default)]
    #[allow(dead_code)]
    pub testing: GetStartedReleaseChannel,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct GetStartedReleaseChannel {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub page_url: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub configured: bool,
}

pub struct ConsoleClient {
    client: Client,
    base_url: String,
    token_store: TokenStore,
}

impl ConsoleClient {
    pub fn new(base_url: impl Into<String>, token_store: TokenStore) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url: base_url.into(),
            token_store,
        }
    }

    pub fn is_logged_in(&self) -> bool {
        match self.token_store.load() {
            Ok(Some(token)) => !token.is_expired(),
            _ => false,
        }
    }

    fn load_valid_token(&self) -> anyhow::Result<String> {
        let token = self
            .token_store
            .load()?
            .ok_or_else(|| anyhow::anyhow!("未登录，请先执行 easytier-pro-installer login"))?;
        if token.is_expired() {
            anyhow::bail!("登录已过期，请重新执行 easytier-pro-installer login")
        }
        Ok(token.access_token)
    }

    pub async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
    ) -> anyhow::Result<T> {
        self.send(method, path, |req| req).await
    }

    pub async fn request_with_body<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        method: Method,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        self.send(method, path, |req| req.json(body)).await
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        configure: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> anyhow::Result<T> {
        let access_token = self.load_valid_token()?;
        let resp = configure(
            self.client
                .request(method, format!("{}{}", self.base_url, path))
                .bearer_auth(&access_token),
        )
        .send()
        .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("请求失败: {}", text)
        }

        Ok(resp.json().await?)
    }

    pub async fn get_me(&self) -> anyhow::Result<MeResponse> {
        self.request(Method::GET, "/api/v1/auth/me").await
    }

    pub async fn list_device_enrollment_keys(
        &self,
        tenant_id: &str,
    ) -> anyhow::Result<Vec<DeviceEnrollmentKey>> {
        self.request(
            Method::GET,
            &format!("/api/v1/tenants/{}/device-enrollment-keys", tenant_id),
        )
        .await
    }

    pub async fn create_device_enrollment_key(
        &self,
        tenant_id: &str,
        req: &CreateDeviceEnrollmentKeyRequest,
    ) -> anyhow::Result<CreateDeviceEnrollmentKeyResponse> {
        self.request_with_body(
            Method::POST,
            &format!("/api/v1/tenants/{}/device-enrollment-keys", tenant_id),
            req,
        )
        .await
    }

    pub async fn get_device_enrollment_key_secret(
        &self,
        tenant_id: &str,
        key_id: &str,
    ) -> anyhow::Result<DeviceEnrollmentKeySecret> {
        self.request(
            Method::GET,
            &format!(
                "/api/v1/tenants/{}/device-enrollment-keys/{}/secret",
                tenant_id, key_id
            ),
        )
        .await
    }

    pub async fn get_started(&self, tenant_id: &str) -> anyhow::Result<GetStartedResponse> {
        self.request(
            Method::GET,
            &format!("/api/v1/tenants/{}/get-started", tenant_id),
        )
        .await
    }
}
