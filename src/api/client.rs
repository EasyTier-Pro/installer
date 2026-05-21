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
pub struct LatestReleaseResponse {
    #[serde(default)]
    pub stable: ReleaseChannelInfo,
    #[serde(default)]
    #[allow(dead_code)]
    pub testing: ReleaseChannelInfo,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ReleaseChannelInfo {
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct DeviceSummary {
    pub id: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub approval_state: String,
    #[serde(default)]
    pub connectivity_state: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct NetworkSummary {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ipv4_cidr: String,
    #[serde(default)]
    pub node_ipv4: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct MachineStatusResponse {
    pub device: DeviceSummary,
    #[serde(default)]
    pub networks: Vec<NetworkSummary>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct Network {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub network_name: String,
    #[serde(default)]
    pub ipv4_cidr: String,
    #[serde(default)]
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateNetworkRequest {
    pub name: String,
    pub regions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4_cidr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_traffic_quota_bytes: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateNodeRequest {
    pub device_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct NodeOperationResult {
    #[serde(default)]
    pub node: Option<NetworkNode>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct NetworkNode {
    pub id: String,
    #[serde(default)]
    pub ipv4_addr: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub ipv4_addr: Option<String>,
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
        if crate::style::debug_enabled()
            && let Ok(json) = serde_json::to_string(body)
        {
            crate::style::debug(&format!("API 请求 body: {}", json));
        }
        self.send(method, path, |req| req.json(body)).await
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        configure: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> anyhow::Result<T> {
        let access_token = self.load_valid_token()?;
        let url = format!("{}{}", self.base_url, path);
        crate::style::debug(&format!("API 请求: {} {}", method, url));

        let resp = configure(self.client.request(method, &url).bearer_auth(&access_token))
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        crate::style::debug(&format!("API 响应 [{}]: {}", status, text));

        if !status.is_success() {
            anyhow::bail!("请求失败: {}", text)
        }

        Ok(serde_json::from_str(&text)?)
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

    pub async fn get_latest_release(&self) -> anyhow::Result<LatestReleaseResponse> {
        self.request_public(Method::GET, "/api/v1/releases/latest")
            .await
    }

    pub async fn get_machine_status(
        &self,
        tenant_id: &str,
        machine_id: &str,
    ) -> anyhow::Result<MachineStatusResponse> {
        self.request(
            Method::GET,
            &format!("/api/v1/tenants/{}/machines/{}", tenant_id, machine_id),
        )
        .await
    }

    pub async fn list_networks(&self, tenant_id: &str) -> anyhow::Result<Vec<Network>> {
        self.request(
            Method::GET,
            &format!("/api/v1/tenants/{}/networks", tenant_id),
        )
        .await
    }

    pub async fn request_public<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
    ) -> anyhow::Result<T> {
        let url = format!("{}{}", self.base_url, path);
        crate::style::debug(&format!("API 公共请求: {} {}", method, url));

        let resp = self.client.request(method, &url).send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        crate::style::debug(&format!("API 公共响应 [{}]: {}", status, text));

        if !status.is_success() {
            anyhow::bail!("请求失败: {}", text)
        }

        Ok(serde_json::from_str(&text)?)
    }

    pub async fn create_network(
        &self,
        tenant_id: &str,
        req: &CreateNetworkRequest,
    ) -> anyhow::Result<Network> {
        self.request_with_body(
            Method::POST,
            &format!("/api/v1/tenants/{}/networks", tenant_id),
            req,
        )
        .await
    }

    pub async fn create_node(
        &self,
        tenant_id: &str,
        network_id: &str,
        req: &CreateNodeRequest,
    ) -> anyhow::Result<NodeOperationResult> {
        self.request_with_body(
            Method::POST,
            &format!(
                "/api/v1/tenants/{}/networks/{}/nodes",
                tenant_id, network_id
            ),
            req,
        )
        .await
    }

    pub async fn get_network_nodes(
        &self,
        tenant_id: &str,
        network_id: &str,
    ) -> anyhow::Result<Vec<Node>> {
        self.request(
            Method::GET,
            &format!(
                "/api/v1/tenants/{}/networks/{}/nodes",
                tenant_id, network_id
            ),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;

    #[tokio::test]
    async fn get_latest_release_uses_public_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("local addr");
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).expect("read request");
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let _ = tx.send(request);
            let body = "{\"stable\":{\"version\":\"v1\"}}";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let client = ConsoleClient::new(
            format!("http://{}", addr),
            TokenStore::new(PathBuf::from("/tmp/easytier-pro-installer-test-token.json")),
        );
        let release = client.get_latest_release().await.expect("release");

        assert_eq!(release.stable.version, "v1");
        let request = rx.recv().expect("request");
        assert!(request.starts_with("GET /api/v1/releases/latest HTTP/1.1"));
        assert!(!request.contains("Authorization:"));
    }
}
