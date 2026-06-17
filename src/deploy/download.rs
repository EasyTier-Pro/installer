use crate::deploy::platform::{Platform, default_cache_dir};
use anyhow::Context;
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const GITHUB_RELEASE_API: &str = "https://api.github.com/repos/EasyTier/EasyTier/releases/tags";
const USER_AGENT: &str = "easytier-pro-installer";

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

fn checksum_metadata_url(version: &str) -> String {
    format!("{}/{}", GITHUB_RELEASE_API, normalize_version(version))
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
    download_easytier_with_timeout_impl(
        platform,
        install_dir,
        version,
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
    let core_path = install_dir.join(core_name);
    let cli_path = install_dir.join(cli_name);
    let version_file = install_dir.join(".version");

    if core_path.exists() && cli_path.exists() {
        let cached = std::fs::read_to_string(&version_file)
            .unwrap_or_default()
            .trim()
            .to_string();
        if cached == version {
            if !quiet {
                crate::style::info(&format!("本地已有 {}，跳过下载", version.bright_white()));
            }
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
    let expected_sha256 =
        fetch_expected_sha256(&client, &version, &asset_name, connect_timeout_secs)
            .await
            .with_context(|| format!("无法获取 {} 的 SHA-256 校验信息", asset_name))?;

    let zip_data = if let Some(zip_data) =
        read_verified_cache(&archive_path, &expected_sha256, &asset_name, quiet)?
    {
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

        verify_sha256(&zip_data, &expected_sha256, &asset_name)?;
        std::fs::write(&archive_path, &zip_data)?;
        if !quiet {
            crate::style::info(&format!(
                "已缓存压缩包到 {}",
                archive_path.to_string_lossy().bright_white()
            ));
        }
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

async fn fetch_expected_sha256(
    client: &reqwest::Client,
    version: &str,
    asset_name: &str,
    connect_timeout_secs: u64,
) -> anyhow::Result<String> {
    // EasyTier core releases expose authoritative per-asset checksums in the
    // GitHub release metadata `digest` field (`sha256:<hex>`). Installer
    // bootstrap binaries use sidecar `<asset>.sha256` checksum files.
    let checksum_url = checksum_metadata_url(version);
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(connect_timeout_secs),
        client
            .get(&checksum_url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("校验信息请求超时"))??;

    if !resp.status().is_success() {
        anyhow::bail!("校验信息下载失败 ({})", resp.status());
    }

    let metadata = resp.text().await?;
    parse_github_release_sha256(&metadata, asset_name)
}

fn read_verified_cache(
    archive_path: &Path,
    expected_sha256: &str,
    asset_name: &str,
    quiet: bool,
) -> anyhow::Result<Option<Vec<u8>>> {
    if !archive_path.exists() {
        return Ok(None);
    }

    let zip_data = std::fs::read(archive_path)?;
    match verify_sha256(&zip_data, expected_sha256, asset_name) {
        Ok(()) => {
            if !quiet {
                crate::style::info(&format!(
                    "使用已验证缓存压缩包 {}",
                    archive_path.to_string_lossy().bright_white()
                ));
            }
            Ok(Some(zip_data))
        }
        Err(err) => {
            if !quiet {
                crate::style::warning(&format!("缓存压缩包校验失败，重新下载: {}", err));
            }
            std::fs::remove_file(archive_path).with_context(|| {
                format!("无法删除校验失败的缓存文件 {}", archive_path.display())
            })?;
            Ok(None)
        }
    }
}

fn parse_github_release_sha256(metadata: &str, asset_name: &str) -> anyhow::Result<String> {
    let release: serde_json::Value = serde_json::from_str(metadata)?;
    let assets = release
        .get("assets")
        .and_then(|assets| assets.as_array())
        .ok_or_else(|| anyhow::anyhow!("release metadata 缺少 assets"))?;

    for asset in assets {
        let name = asset.get("name").and_then(|name| name.as_str());
        if name != Some(asset_name) {
            continue;
        }

        let digest = asset
            .get("digest")
            .and_then(|digest| digest.as_str())
            .ok_or_else(|| anyhow::anyhow!("{} 缺少 digest 字段", asset_name))?;
        let expected = digest
            .strip_prefix("sha256:")
            .ok_or_else(|| anyhow::anyhow!("{} digest 不是 SHA-256", asset_name))?;
        validate_sha256_hex(expected)?;
        return Ok(expected.to_ascii_lowercase());
    }

    anyhow::bail!("release metadata 中未找到 {}", asset_name)
}

fn verify_sha256(data: &[u8], expected_sha256: &str, asset_name: &str) -> anyhow::Result<()> {
    validate_sha256_hex(expected_sha256)?;
    let actual = sha256_hex(data);
    if actual != expected_sha256.to_ascii_lowercase() {
        anyhow::bail!(
            "{} SHA-256 mismatch: expected {}, got {}",
            asset_name,
            expected_sha256,
            actual
        );
    }
    Ok(())
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn validate_sha256_hex(value: &str) -> anyhow::Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("无效的 SHA-256 校验值: {}", value);
    }
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

    fn test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();

        for (path, contents) in entries {
            archive.start_file(path, options).unwrap();
            archive.write_all(contents).unwrap();
        }

        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn normalizes_version_prefix() {
        assert_eq!(normalize_version("2.6.4"), "v2.6.4");
        assert_eq!(normalize_version("v2.6.4"), "v2.6.4");
    }

    #[test]
    fn builds_asset_name_for_macos() {
        let platform = Platform {
            os: "darwin",
            arch: "aarch64",
        };

        assert_eq!(
            asset_name(&platform, "2.6.4"),
            "easytier-macos-aarch64-v2.6.4.zip"
        );
    }

    #[test]
    fn builds_asset_name_with_platform_arch() {
        let platform = Platform {
            os: "linux",
            arch: "riscv64",
        };

        assert_eq!(
            asset_name(&platform, "v2.6.4"),
            "easytier-linux-riscv64-v2.6.4.zip"
        );
    }

    #[test]
    fn builds_download_urls_for_known_sources() {
        let platform = Platform {
            os: "linux",
            arch: "x86_64",
        };

        assert_eq!(
            build_download_url(&platform, "v2.6.4", "github"),
            "https://github.com/EasyTier/EasyTier/releases/download/v2.6.4/easytier-linux-x86_64-v2.6.4.zip"
        );
        assert_eq!(
            build_download_url(&platform, "v2.6.4", "github_proxy"),
            "https://ghfast.top/https://github.com/EasyTier/EasyTier/releases/download/v2.6.4/easytier-linux-x86_64-v2.6.4.zip"
        );
        assert_eq!(
            build_download_url(&platform, "v2.6.4", "github-proxy"),
            "https://ghfast.top/https://github.com/EasyTier/EasyTier/releases/download/v2.6.4/easytier-linux-x86_64-v2.6.4.zip"
        );
        assert_eq!(
            build_download_url(&platform, "v2.6.4", "gitee"),
            "https://gitee.com/EasyTier/EasyTier/releases/download/v2.6.4/easytier-linux-x86_64-v2.6.4.zip"
        );
    }

    #[test]
    fn extract_zip_finds_package_root_at_archive_root() {
        let extract_dir = temp_test_path("extract-root");
        std::fs::create_dir_all(&extract_dir).unwrap();
        let core_name = crate::deploy::core_binary_name();
        let cli_name = crate::deploy::cli_binary_name();
        let archive = test_zip(&[(core_name, b"core".as_ref()), (cli_name, b"cli".as_ref())]);

        extract_zip(&archive, &extract_dir).unwrap();
        let package_root = find_package_root(&extract_dir, core_name, cli_name).unwrap();

        assert_eq!(package_root, Some(extract_dir.clone()));
        assert_eq!(
            std::fs::read_to_string(extract_dir.join(core_name)).unwrap(),
            "core"
        );
        assert_eq!(
            std::fs::read_to_string(extract_dir.join(cli_name)).unwrap(),
            "cli"
        );
        let _ = std::fs::remove_dir_all(extract_dir.parent().unwrap());
    }

    #[test]
    fn extract_zip_finds_package_root_in_top_level_directory() {
        let extract_dir = temp_test_path("extract-nested");
        std::fs::create_dir_all(&extract_dir).unwrap();
        let core_name = crate::deploy::core_binary_name();
        let cli_name = crate::deploy::cli_binary_name();
        let core_path = format!("package/{core_name}");
        let cli_path = format!("package/{cli_name}");
        let archive = test_zip(&[(&core_path, b"core".as_ref()), (&cli_path, b"cli".as_ref())]);

        extract_zip(&archive, &extract_dir).unwrap();
        let package_root = find_package_root(&extract_dir, core_name, cli_name).unwrap();

        assert_eq!(package_root, Some(extract_dir.join("package")));
        let _ = std::fs::remove_dir_all(extract_dir.parent().unwrap());
    }

    #[test]
    fn extract_zip_rejects_invalid_zip_bytes() {
        let extract_dir = temp_test_path("extract-invalid");
        std::fs::create_dir_all(&extract_dir).unwrap();

        let err = extract_zip(b"not a zip", &extract_dir).unwrap_err();

        assert!(!err.to_string().is_empty());
        let _ = std::fs::remove_dir_all(extract_dir.parent().unwrap());
    }

    #[test]
    fn find_package_root_returns_none_when_binaries_are_missing() {
        let extract_dir = temp_test_path("extract-missing");
        std::fs::create_dir_all(&extract_dir).unwrap();
        let core_name = crate::deploy::core_binary_name();
        let cli_name = crate::deploy::cli_binary_name();

        let package_root = find_package_root(&extract_dir, core_name, cli_name).unwrap();

        assert_eq!(package_root, None);
        let _ = std::fs::remove_dir_all(extract_dir.parent().unwrap());
    }

    #[test]
    fn sync_dir_contents_copies_nested_overwrites_and_preserves_files() {
        let root = temp_test_path("sync-dir-contents");
        let src = root.join("src");
        let dst = root.join("dst");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("nested/copied.txt"), "copied").unwrap();
        std::fs::write(src.join("overwritten.txt"), "new").unwrap();
        std::fs::write(dst.join("overwritten.txt"), "old").unwrap();
        std::fs::write(dst.join("preserved.txt"), "preserved").unwrap();

        sync_dir_contents(&src, &dst).unwrap();

        assert_eq!(
            std::fs::read_to_string(dst.join("nested/copied.txt")).unwrap(),
            "copied"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("overwritten.txt")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("preserved.txt")).unwrap(),
            "preserved"
        );
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn parses_matching_github_release_checksum() {
        let metadata = r#"{
            "assets": [
                {
                    "name": "easytier-linux-x86_64-v2.6.4.zip",
                    "digest": "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                }
            ]
        }"#;

        let checksum =
            parse_github_release_sha256(metadata, "easytier-linux-x86_64-v2.6.4.zip").unwrap();

        assert_eq!(
            checksum,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn rejects_checksum_mismatch() {
        let expected = "0000000000000000000000000000000000000000000000000000000000000000";

        let err = verify_sha256(b"abc", expected, "archive.zip").unwrap_err();

        assert!(err.to_string().contains("SHA-256 mismatch"));
    }

    #[test]
    fn accepts_matching_checksum() {
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        verify_sha256(b"abc", expected, "archive.zip").unwrap();
    }

    #[test]
    fn rejects_and_removes_corrupted_cache() {
        let path = temp_test_path("archive.zip");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"corrupted").unwrap();
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        let cached = read_verified_cache(&path, expected, "archive.zip", true).unwrap();

        assert!(cached.is_none());
        assert!(!path.exists());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
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
