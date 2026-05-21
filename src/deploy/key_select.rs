use crate::api::client::{ConsoleClient, CreateDeviceEnrollmentKeyRequest, DeviceEnrollmentKey};

pub(crate) fn key_name(key: &DeviceEnrollmentKey) -> &str {
    key.display_name.as_deref().unwrap_or(&key.key_code)
}

pub(crate) fn key_type_label(reusable: bool) -> &'static str {
    if reusable { "多设备" } else { "单设备" }
}

pub(crate) fn confirm_yes(prompt: &str) -> anyhow::Result<bool> {
    dialoguer::Confirm::with_theme(&crate::style::dialoguer_theme())
        .with_prompt(prompt)
        .default(true)
        .interact()
        .map_err(|e| e.into())
}

pub(crate) fn read_choice(items: &[String], prompt: &str) -> anyhow::Result<usize> {
    let selection = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact()?;
    Ok(selection + 1)
}

pub(crate) async fn get_key_token(
    client: &ConsoleClient,
    tenant_id: &str,
    key: &DeviceEnrollmentKey,
) -> anyhow::Result<String> {
    Ok(client
        .get_device_enrollment_key_secret(tenant_id, &key.id)
        .await?
        .bootstrap_token)
}

pub(crate) fn is_key_secret_unavailable(err: &anyhow::Error) -> bool {
    let text = err.to_string();
    text.contains("device_enrollment_key_secret_unavailable")
        || text.contains("key secret unavailable")
}

pub(crate) async fn select_key(
    client: &ConsoleClient,
    tenant_id: &str,
    multi_keys: &[DeviceEnrollmentKey],
    single_keys: &[DeviceEnrollmentKey],
) -> anyhow::Result<(String, DeviceEnrollmentKey)> {
    loop {
        let mut options: Vec<String> = Vec::new();
        let mut key_refs: Vec<&DeviceEnrollmentKey> = Vec::new();

        for key in multi_keys {
            options.push(format!("{} [多设备]", key_name(key)));
            key_refs.push(key);
        }
        for key in single_keys {
            options.push(format!("{} [单设备]", key_name(key)));
            key_refs.push(key);
        }

        options.push("[创建新密钥]".to_string());

        let choice = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
            .with_prompt("请选择要使用的注册密钥")
            .items(&options)
            .default(0)
            .interact()?;

        if choice == key_refs.len() {
            let (key, token) = create_new_key(client, tenant_id).await?;
            let label = key_type_label(key.reusable);
            crate::style::success(&format!("已创建{}密钥: {}", label, key_name(&key)));
            return Ok((token, key));
        }

        let key = key_refs[choice].clone();
        match get_key_token(client, tenant_id, &key).await {
            Ok(token) => return Ok((token, key)),
            Err(err) if is_key_secret_unavailable(&err) => {
                crate::style::warning(&format!(
                    "密钥 {} 当前无法用于部署，请选择其他密钥或创建新密钥。",
                    key_name(&key)
                ));
            }
            Err(err) => return Err(err),
        }
    }
}

pub(crate) async fn create_new_key(
    client: &ConsoleClient,
    tenant_id: &str,
) -> anyhow::Result<(DeviceEnrollmentKey, String)> {
    let default_name = format!(
        "agent-{}",
        std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "unknown".to_string())
    );

    let name = dialoguer::Input::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("请输入新密钥的名称")
        .default(default_name)
        .interact()?;

    let type_items = vec!["单设备（仅本设备可用）", "多设备（可被多台设备共用）"];
    let is_multi = dialoguer::Select::with_theme(&crate::style::dialoguer_theme())
        .with_prompt("该密钥是否可被多台设备共用")
        .items(&type_items)
        .default(0)
        .interact()?
        == 1;

    let req = CreateDeviceEnrollmentKeyRequest {
        display_name: Some(name),
        tags: None,
        reusable: is_multi,
        pre_approved: true,
    };

    let resp = client.create_device_enrollment_key(tenant_id, &req).await?;
    Ok((resp.enrollment_key, resp.bootstrap_token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unavailable_key_secret_errors() {
        let err = anyhow::Error::msg(
            r#"请求失败: {"code":"device_enrollment_key_secret_unavailable","error":"key secret unavailable"}"#,
        );

        assert!(is_key_secret_unavailable(&err));
    }

    #[test]
    fn ignores_other_key_errors() {
        let err = anyhow::Error::msg(r#"请求失败: {"code":"unauthorized","error":"unauthorized"}"#);

        assert!(!is_key_secret_unavailable(&err));
    }
}
