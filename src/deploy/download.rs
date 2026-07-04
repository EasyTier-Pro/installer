use crate::deploy::platform::{Platform, default_cache_dir};
use colored::Colorize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub(crate) struct DownloadProgress {
    pub(crate) downloaded: u64,
    pub(crate) total: Option<u64>,
}

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
    let mut ignore_progress = |_progress: DownloadProgress| Ok(());
    download_easytier_with_fallback_impl(
        platform,
        install_dir,
        version,
        &mut ignore_progress,
        false,
    )
    .await
}

pub(crate) async fn download_easytier_with_fallback_report<F>(
    platform: &Platform,
    install_dir: &Path,
    version: &str,
    on_progress: &mut F,
) -> anyhow::Result<(PathBuf, PathBuf, String)>
where
    F: FnMut(DownloadProgress) -> anyhow::Result<()>,
{
    download_easytier_with_fallback_impl(platform, install_dir, version, on_progress, true).await
}

async fn download_easytier_with_fallback_impl<F>(
    platform: &Platform,
    install_dir: &Path,
    version: &str,
    on_progress: &mut F,
    quiet: bool,
) -> anyhow::Result<(PathBuf, PathBuf, String)>
where
    F: FnMut(DownloadProgress) -> anyhow::Result<()>,
{
    let sources = [("gitee", 10u64), ("github_proxy", 10u64), ("github", 10u64)];
    let version = normalize_version(version);
    let core_name = super::core_binary_name();
    let cli_name = super::cli_binary_name();

    if let Some(result) = existing_install_result(install_dir, core_name, cli_name, &version, quiet)
    {
        return Ok(result);
    }

    if let Some(result) = try_install_bundled_archive(
        platform,
        install_dir,
        &version,
        core_name,
        cli_name,
        on_progress,
        quiet,
    )? {
        return Ok(result);
    }

    for (source, connect_timeout_secs) in &sources {
        let url = build_download_url(platform, &version, source);
        if !quiet {
            crate::style::info(&format!("尝试从 {} 下载...", source.bright_white()));
        }
        match download_easytier_with_timeout_impl(
            platform,
            install_dir,
            &version,
            &url,
            *connect_timeout_secs,
            on_progress,
            quiet,
        )
        .await
        {
            Ok(result) => return Ok(result),
            Err(e) => {
                if !quiet {
                    crate::style::warning(&format!("{} 不可用: {}", source, e));
                }
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
    let mut ignore_progress = |_progress: DownloadProgress| Ok(());
    let version = normalize_version(version);
    let core_name = super::core_binary_name();
    let cli_name = super::cli_binary_name();

    if let Some(result) = existing_install_result(install_dir, core_name, cli_name, &version, false)
    {
        return Ok(result);
    }

    if let Some(result) = try_install_bundled_archive(
        platform,
        install_dir,
        &version,
        core_name,
        cli_name,
        &mut ignore_progress,
        false,
    )? {
        return Ok(result);
    }

    download_easytier_with_timeout_impl(
        platform,
        install_dir,
        &version,
        download_url,
        connect_timeout_secs,
        &mut ignore_progress,
        false,
    )
    .await
}

async fn download_easytier_with_timeout_impl<F>(
    platform: &Platform,
    install_dir: &Path,
    version: &str,
    download_url: &str,
    connect_timeout_secs: u64,
    on_progress: &mut F,
    quiet: bool,
) -> anyhow::Result<(PathBuf, PathBuf, String)>
where
    F: FnMut(DownloadProgress) -> anyhow::Result<()>,
{
    let version = normalize_version(version);

    let core_name = super::core_binary_name();
    let cli_name = super::cli_binary_name();

    if let Some(result) = existing_install_result(install_dir, core_name, cli_name, &version, quiet)
    {
        return Ok(result);
    }

    let core_path = install_dir.join(core_name);
    let cli_path = install_dir.join(cli_name);
    let asset_name = asset_name(platform, &version);
    let cache_dir = default_cache_dir();
    let archive_path = cache_dir.join(&asset_name);

    std::fs::create_dir_all(&cache_dir)?;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
        .build()?;

    let zip_data = if archive_path.exists() {
        if !quiet {
            crate::style::info(&format!(
                "使用缓存压缩包 {}",
                archive_path.to_string_lossy().bright_white()
            ));
        }
        let zip_data = std::fs::read(&archive_path)?;
        let size = zip_data.len() as u64;
        on_progress(DownloadProgress {
            downloaded: size,
            total: Some(size),
        })?;
        zip_data
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
        let pb = if quiet {
            None
        } else {
            let pb = indicatif::ProgressBar::new(total_size);
            pb.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                    .progress_chars("#>-"),
            );
            Some(pb)
        };

        let mut zip_data = Vec::new();
        let mut stream = resp.bytes_stream();
        use tokio_stream::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            zip_data.extend_from_slice(&chunk);
            if let Some(pb) = &pb {
                pb.inc(chunk.len() as u64);
            }
            on_progress(DownloadProgress {
                downloaded: zip_data.len() as u64,
                total: (total_size > 0).then_some(total_size),
            })?;
        }
        if let Some(pb) = &pb {
            pb.finish_and_clear();
        }

        std::fs::write(&archive_path, &zip_data)?;
        if !quiet {
            crate::style::info(&format!(
                "已缓存压缩包到 {}",
                archive_path.to_string_lossy().bright_white()
            ));
        }
        zip_data
    };

    let (staging_dir, package_root) = stage_archive(&zip_data, install_dir, core_name, cli_name)?;
    install_staged_archive(
        &staging_dir,
        &package_root,
        install_dir,
        &core_path,
        &cli_path,
        &version,
    )?;
    Ok((core_path, cli_path, version))
}

fn existing_install_result(
    install_dir: &Path,
    core_name: &str,
    cli_name: &str,
    version: &str,
    quiet: bool,
) -> Option<(PathBuf, PathBuf, String)> {
    let core_path = install_dir.join(core_name);
    let cli_path = install_dir.join(cli_name);
    let version_file = install_dir.join(".version");

    if !core_path.exists() || !cli_path.exists() {
        return None;
    }

    let cached = std::fs::read_to_string(&version_file)
        .unwrap_or_default()
        .trim()
        .to_string();
    if cached != version {
        return None;
    }

    if !quiet {
        crate::style::info(&format!("本地已有 {}，跳过下载", version.bright_white()));
    }
    Some((core_path, cli_path, version.to_string()))
}

fn try_install_bundled_archive<F>(
    platform: &Platform,
    install_dir: &Path,
    version: &str,
    core_name: &str,
    cli_name: &str,
    on_progress: &mut F,
    quiet: bool,
) -> anyhow::Result<Option<(PathBuf, PathBuf, String)>>
where
    F: FnMut(DownloadProgress) -> anyhow::Result<()>,
{
    let asset_name = asset_name(platform, version);
    let Some(zip_data) = read_bundled_archive(&asset_name, quiet)? else {
        return Ok(None);
    };

    let (staging_dir, package_root) =
        match stage_archive(&zip_data, install_dir, core_name, cli_name) {
            Ok(staged) => staged,
            Err(err) => {
                if !quiet {
                    crate::style::warning(&format!(
                        "内置压缩包不可用，继续尝试缓存或下载: {}",
                        err
                    ));
                }
                return Ok(None);
            }
        };

    let size = zip_data.len() as u64;
    if let Err(err) = on_progress(DownloadProgress {
        downloaded: size,
        total: Some(size),
    }) {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(err);
    }

    let core_path = install_dir.join(core_name);
    let cli_path = install_dir.join(cli_name);
    install_staged_archive(
        &staging_dir,
        &package_root,
        install_dir,
        &core_path,
        &cli_path,
        version,
    )?;

    Ok(Some((core_path, cli_path, version.to_string())))
}

fn read_bundled_archive(asset_name: &str, quiet: bool) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(bundled_archive_path) = bundled_archive_path(asset_name) else {
        return Ok(None);
    };

    if !quiet {
        crate::style::info(&format!(
            "使用内置压缩包 {}",
            bundled_archive_path.to_string_lossy().bright_white()
        ));
    }

    match std::fs::read(&bundled_archive_path) {
        Ok(zip_data) => Ok(Some(zip_data)),
        Err(err) => {
            if !quiet {
                crate::style::warning(&format!("内置压缩包读取失败，忽略: {}", err));
            }
            Ok(None)
        }
    }
}

fn bundled_archive_path(asset_name: &str) -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let archive_path = archive_path_next_to_exe(&current_exe, asset_name)?;
    archive_path.exists().then_some(archive_path)
}

fn archive_path_next_to_exe(exe_path: &Path, asset_name: &str) -> Option<PathBuf> {
    exe_path.parent().map(|dir| dir.join(asset_name))
}

fn stage_archive(
    zip_data: &[u8],
    install_dir: &Path,
    core_name: &str,
    cli_name: &str,
) -> anyhow::Result<(PathBuf, PathBuf)> {
    let staging_dir = install_dir.join(".extract-tmp");
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir)?;
    }
    std::fs::create_dir_all(&staging_dir)?;

    let result = (|| {
        extract_zip(zip_data, &staging_dir)?;
        find_package_root(&staging_dir, core_name, cli_name)?
            .ok_or_else(|| anyhow::anyhow!("解压后未找到 easytier-core 和 easytier-cli"))
    })();

    match result {
        Ok(package_root) => Ok((staging_dir, package_root)),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            Err(err)
        }
    }
}

fn install_staged_archive(
    staging_dir: &Path,
    package_root: &Path,
    install_dir: &Path,
    core_path: &Path,
    cli_path: &Path,
    version: &str,
) -> anyhow::Result<()> {
    sync_dir_contents(package_root, install_dir)?;
    std::fs::remove_dir_all(staging_dir)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(core_path, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(cli_path, std::fs::Permissions::from_mode(0o755))?;
    }

    std::fs::write(install_dir.join(".version"), version)?;
    Ok(())
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

fn find_package_root(
    extract_dir: &Path,
    core_name: &str,
    cli_name: &str,
) -> anyhow::Result<Option<PathBuf>> {
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

pub(crate) fn sync_dir_contents(src: &Path, dst: &Path) -> anyhow::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_test_path(name: &str) -> PathBuf {
        let unique = format!(
            "easytier-download-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique).join(name)
    }

    #[test]
    fn archive_path_next_to_exe_uses_installer_directory() {
        let exe_path = Path::new("install-dir")
            .join("resources")
            .join("easytier-pro-installer.exe");

        let archive_path =
            archive_path_next_to_exe(&exe_path, "easytier-windows-x86_64-v2.6.4.zip")
                .expect("archive path");

        assert_eq!(
            archive_path,
            PathBuf::from("install-dir")
                .join("resources")
                .join("easytier-windows-x86_64-v2.6.4.zip")
        );
    }

    #[test]
    fn stage_archive_removes_invalid_staging_dir() {
        let install_dir = temp_test_path("install");
        std::fs::create_dir_all(&install_dir).unwrap();

        let err =
            stage_archive(b"not a zip", &install_dir, "easytier-core", "easytier-cli").unwrap_err();

        assert!(!err.to_string().is_empty());
        assert!(!install_dir.join(".extract-tmp").exists());
        std::fs::remove_dir_all(install_dir.parent().unwrap()).unwrap();
    }
}
