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

pub(crate) fn asset_name(platform: &Platform, version: &str) -> String {
    let os = match platform.os {
        "darwin" => "macos",
        other => other,
    };
    format!(
        "easytier-{}-{}-{}.zip",
        os,
        platform.arch,
        normalize_version(version)
    )
}

pub(crate) fn build_download_url(platform: &Platform, version: &str, source: &str) -> String {
    let asset_name = asset_name(platform, version);
    match source.to_lowercase().as_str() {
        "github" => format!(
            "https://github.com/EasyTier/EasyTier/releases/download/{}/{}",
            version, asset_name
        ),
        "github_proxy" | "github-proxy" => format!(
            "https://ghfast.top/https://github.com/EasyTier/EasyTier/releases/download/{}/{}",
            version, asset_name
        ),
        _ => format!(
            "https://gitee.com/EasyTier/EasyTier/releases/download/{}/{}",
            version, asset_name
        ),
    }
}

#[allow(dead_code)]
pub(crate) async fn download_easytier(
    platform: &Platform,
    install_dir: &Path,
    version: &str,
    download_url: &str,
) -> anyhow::Result<(PathBuf, PathBuf, String)> {
    download_easytier_with_timeout(platform, install_dir, version, download_url, 300).await
}

pub(crate) async fn download_easytier_with_fallback(
    platform: &Platform,
    install_dir: &Path,
    version: &str,
) -> anyhow::Result<(PathBuf, PathBuf, String)> {
    let sources = [("gitee", 10u64), ("github_proxy", 10u64), ("github", 10u64)];
    let version = normalize_version(version);

    for (source, connect_timeout_secs) in &sources {
        let url = build_download_url(platform, &version, source);
        crate::style::info(&format!("尝试从 {} 下载...", source.bright_white()));
        match download_easytier_with_timeout(platform, install_dir, &version, &url, *connect_timeout_secs).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                crate::style::warning(&format!("{} 不可用: {}", source, e));
            }
        }
    }

    anyhow::bail!("所有下载源均不可用，请检查网络连接或手动下载")
}

async fn download_easytier_with_timeout(
    platform: &Platform,
    install_dir: &Path,
    version: &str,
    download_url: &str,
    connect_timeout_secs: u64,
) -> anyhow::Result<(PathBuf, PathBuf, String)> {
    let version = normalize_version(version);

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

    let asset_name = asset_name(platform, &version);
    let cache_dir = default_cache_dir();
    let archive_path = cache_dir.join(&asset_name);

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
        .build()?;

    std::fs::create_dir_all(&cache_dir)?;
    let zip_data = if archive_path.exists() {
        crate::style::info(&format!(
            "使用缓存压缩包 {}",
            archive_path.to_string_lossy().bright_white()
        ));
        std::fs::read(&archive_path)?
    } else {
        let resp = tokio::time::timeout(
            std::time::Duration::from_secs(connect_timeout_secs),
            client.get(download_url).send(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("连接超时"))??;
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

    let staging_dir = install_dir.join(".extract-tmp");
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir)?;
    }
    std::fs::create_dir_all(&staging_dir)?;
    extract_zip(&zip_data, &staging_dir)?;

    let package_root = find_package_root(&staging_dir, core_name, cli_name)?
        .ok_or_else(|| anyhow::anyhow!("解压后未找到 easytier-core 和 easytier-cli"))?;
    sync_dir_contents(&package_root, install_dir)?;
    std::fs::remove_dir_all(&staging_dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&core_path, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o755))?;
    }

    std::fs::write(install_dir.join(".version"), &version)?;
    Ok((core_path, cli_path, version))
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

fn find_package_root(extract_dir: &Path, core_name: &str, cli_name: &str) -> anyhow::Result<Option<PathBuf>> {
    if extract_dir.join(core_name).exists() && extract_dir.join(cli_name).exists() {
        return Ok(Some(extract_dir.to_path_buf()));
    }

    for entry in std::fs::read_dir(extract_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join(core_name).exists() && path.join(cli_name).exists() {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

fn sync_dir_contents(src: &Path, dst: &Path) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            sync_dir_contents(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
