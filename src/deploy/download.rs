use crate::deploy::platform::{Platform, default_cache_dir};
use colored::Colorize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(crate) fn normalize_version(version: &str) -> String {
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{}", version)
    }
}

pub(crate) async fn download_easytier(
    platform: &Platform,
    install_dir: &Path,
    version_override: Option<String>,
) -> anyhow::Result<(PathBuf, PathBuf, String)> {
    let is_specific_version = version_override.is_some();
    let version = if let Some(v) = version_override {
        normalize_version(&v)
    } else {
        fetch_latest_version().await?
    };

    let core_name = super::core_binary_name();
    let cli_name = super::cli_binary_name();
    let core_path = install_dir.join(core_name);
    let cli_path = install_dir.join(cli_name);
    let version_file = install_dir.join(".version");

    if core_path.exists() && cli_path.exists() {
        let cached = std::fs::read_to_string(&version_file)
            .unwrap_or_default()
            .trim()
            .to_string();
        if cached == version {
            crate::style::info(&format!("本地已有 {}，跳过下载", version.bright_white()));
            return Ok((core_path, cli_path, version));
        }
    }

    let asset_name = format!("easytier-{}-{}-{}.zip", platform.os, platform.arch, version);
    let cache_dir = default_cache_dir();
    let archive_path = cache_dir.join(&asset_name);
    let download_url = if is_specific_version {
        format!(
            "https://github.com/EasyTier/EasyTier/releases/download/{}/{}",
            version, asset_name
        )
    } else {
        format!(
            "https://github.com/EasyTier/EasyTier/releases/latest/download/{}",
            asset_name
        )
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    std::fs::create_dir_all(&cache_dir)?;
    let zip_data = if archive_path.exists() {
        crate::style::info(&format!(
            "使用缓存压缩包 {}",
            archive_path.to_string_lossy().bright_white()
        ));
        std::fs::read(&archive_path)?
    } else {
        let resp = client.get(&download_url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "下载失败 ({}): 请检查网络连接或手动下载 {} 到 {}",
                resp.status(),
                asset_name,
                cache_dir.display()
            );
        }

        let total_size = resp.content_length().unwrap_or(0);
        let pb = indicatif::ProgressBar::new(total_size);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"),
        );

        let mut zip_data = Vec::new();
        let mut stream = resp.bytes_stream();
        use tokio_stream::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            zip_data.extend_from_slice(&chunk);
            pb.inc(chunk.len() as u64);
        }
        pb.finish_and_clear();

        std::fs::write(&archive_path, &zip_data)?;
        crate::style::info(&format!(
            "已缓存压缩包到 {}",
            archive_path.to_string_lossy().bright_white()
        ));
        zip_data
    };

    extract_zip(&zip_data, install_dir)?;

    let mut found = false;
    for entry in std::fs::read_dir(install_dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            let sub_core = p.join(core_name);
            let sub_cli = p.join(cli_name);
            if sub_core.exists() && sub_cli.exists() {
                std::fs::copy(&sub_core, &core_path)?;
                std::fs::copy(&sub_cli, &cli_path)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&core_path, std::fs::Permissions::from_mode(0o755))?;
                    std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))?;
                }
                found = true;
                break;
            }
        }
    }
    if !found {
        anyhow::bail!("解压后未找到 easytier-core 和 easytier-cli");
    }

    std::fs::write(install_dir.join(".version"), &version)?;
    Ok((core_path, cli_path, version))
}

pub(crate) async fn fetch_latest_version() -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .get("https://api.github.com/repos/EasyTier/EasyTier/releases/latest")
        .header("User-Agent", "easytier-pro-installer/0.1.0")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "无法获取最新版本 ({}): 请使用 --version 指定版本号",
            resp.status()
        );
    }

    let json: serde_json::Value = resp.json().await?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("GitHub API 返回格式异常"))?;
    Ok(tag.to_string())
}

fn extract_zip(data: &[u8], dest: &Path) -> anyhow::Result<()> {
    let reader = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => dest.join(path),
            None => continue,
        };

        if file.is_dir() {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent()
                && !p.exists()
            {
                std::fs::create_dir_all(p)?;
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            let mut buf = [0u8; 8192];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                outfile.write_all(&buf[..n])?;
            }
        }
    }
    Ok(())
}
