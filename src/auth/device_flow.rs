use super::{DeviceAuthInfo, DeviceAuthResponse, OAuthError, TokenExchangeResponse, TokenSet};

fn strip_cancel_token(url: &str) -> String {
    match url.split_once("?cancelToken=") {
        Some((base, _)) => base.to_string(),
        None => url.to_string(),
    }
}
use reqwest::Client;
use std::time::Duration;
use tokio::time::{Instant, interval, sleep};

pub struct DeviceFlow {
    client: Client,
    console_base_url: String,
}

impl DeviceFlow {
    pub fn new(console_base_url: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            console_base_url: console_base_url.into(),
        }
    }

    pub async fn initiate(&self) -> anyhow::Result<DeviceAuthInfo> {
        let resp = self
            .client
            .post(format!("{}/api/v1/auth/device", self.console_base_url))
            .form(&[("client_id", ""), ("scope", "openid profile email")])
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("获取验证码失败: {}", text);
        }

        let body: DeviceAuthResponse = resp.json().await?;
        let verification_uri = strip_cancel_token(&body.verification_uri);
        let verification_uri_complete = body
            .verification_uri_complete
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| verification_uri.clone());
        Ok(DeviceAuthInfo {
            device_code: body.device_code,
            user_code: body.user_code,
            verification_uri,
            verification_uri_complete,
            expires_in: body.expires_in,
            interval: body.interval.max(5),
        })
    }

    pub async fn poll_token(
        &self,
        device_code: &str,
        interval_secs: u64,
        expires_in: u64,
    ) -> anyhow::Result<TokenSet> {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(expires_in);
        let mut tick = interval(Duration::from_secs(interval_secs));

        loop {
            tick.tick().await;

            if Instant::now() > deadline {
                anyhow::bail!("登录超时，请重新执行 login 命令");
            }

            let resp = self
                .client
                .post(format!(
                    "{}/api/v1/auth/device/token",
                    self.console_base_url
                ))
                .form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device_code),
                    ("client_id", ""),
                ])
                .send()
                .await?;

            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();

            if status.is_success() {
                let token: TokenExchangeResponse = serde_json::from_str(&body_text)?;
                return Ok(TokenSet {
                    access_token: token.access_token,
                    id_token: token.id_token,
                    refresh_token: token.refresh_token,
                    token_type: token.token_type,
                    expires_in: token.expires_in,
                    obtained_at: chrono::Utc::now(),
                });
            }

            let err: OAuthError = serde_json::from_str(&body_text).unwrap_or(OAuthError {
                error: "unknown_error".to_string(),
                description: Some(body_text.clone()),
                interval: None,
            });

            match err.error.as_str() {
                "authorization_pending" => continue,
                "slow_down" => {
                    let extra = err.interval.unwrap_or(5);
                    sleep(Duration::from_secs(extra)).await;
                }
                "expired_token" => anyhow::bail!("验证码已过期，请重新执行 login 命令"),
                "access_denied" => anyhow::bail!("用户拒绝了授权"),
                _ => anyhow::bail!("登录失败: {}", err.description.unwrap_or(err.error)),
            }
        }
    }
}
