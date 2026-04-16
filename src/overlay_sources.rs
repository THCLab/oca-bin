use std::fs;
use std::path::{Path, PathBuf};

use oca_sdk_rs::oca::overlay_file::OverlayLocalRegistry;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::CliError;

const DEFAULT_OVERLAY_REPO: &str =
    "https://github.com/the-human-colossus-foundation/overlays-repository";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OverlaySource {
    #[serde(rename = "local")]
    Local { path: PathBuf },
    #[serde(rename = "git")]
    Git { url: String },
}

impl Default for OverlaySource {
    fn default() -> Self {
        OverlaySource::Git {
            url: DEFAULT_OVERLAY_REPO.to_string(),
        }
    }
}

/// Sync overlay sources into a local cache directory, then load a registry from it.
pub fn load_overlay_registry(config: &Config) -> Result<OverlayLocalRegistry, CliError> {
    if config.overlay_sources.is_empty() {
        // Backward-compatible: use existing overlay_definitions_path directly
        return OverlayLocalRegistry::from_dir(&config.overlay_definitions_path).map_err(|e| {
            CliError::OverlayRegistryError(config.overlay_definitions_path.clone(), e)
        });
    }

    let cache_dir = config.overlay_cache_path();
    fs::create_dir_all(&cache_dir).map_err(|e| {
        CliError::OverlaySyncError(
            cache_dir.display().to_string(),
            format!("failed to create cache dir: {}", e),
        )
    })?;

    // Always include the built-in overlay definitions
    copy_overlayfiles_from_dir(&config.overlay_definitions_path, &cache_dir, "builtin")?;

    for source in &config.overlay_sources {
        match source {
            OverlaySource::Local { path } => {
                let prefix = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "local".to_string());
                copy_overlayfiles_from_dir(path, &cache_dir, &prefix)?;
            }
            OverlaySource::Git { url } => {
                fetch_github_overlayfiles(url, &cache_dir)?;
            }
        }
    }

    OverlayLocalRegistry::from_dir(&cache_dir)
        .map_err(|e| CliError::OverlayRegistryError(cache_dir, e))
}

/// Explicitly sync overlay sources and print results.
pub fn sync_overlay_sources(config: &Config) -> Result<PathBuf, CliError> {
    let cache_dir = config.overlay_cache_path();
    fs::create_dir_all(&cache_dir).map_err(|e| {
        CliError::OverlaySyncError(
            cache_dir.display().to_string(),
            format!("failed to create cache dir: {}", e),
        )
    })?;

    // Clear existing cached files so removed upstream files don't linger
    for entry in fs::read_dir(&cache_dir).map_err(|e| {
        CliError::OverlaySyncError(cache_dir.display().to_string(), e.to_string())
    })? {
        let entry = entry.map_err(|e| {
            CliError::OverlaySyncError(cache_dir.display().to_string(), e.to_string())
        })?;
        if entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .map_or(false, |ext| ext == "overlayfile")
        {
            let _ = fs::remove_file(entry.path());
        }
    }

    // Copy built-in definitions
    copy_overlayfiles_from_dir(&config.overlay_definitions_path, &cache_dir, "builtin")?;

    for source in &config.overlay_sources {
        match source {
            OverlaySource::Local { path } => {
                println!("Syncing local source: {}", path.display());
                let prefix = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "local".to_string());
                copy_overlayfiles_from_dir(path, &cache_dir, &prefix)?;
            }
            OverlaySource::Git { url } => {
                println!("Syncing git source: {}", url);
                fetch_github_overlayfiles(url, &cache_dir)?;
            }
        }
    }

    Ok(cache_dir)
}

fn copy_overlayfiles_from_dir(
    src_dir: &Path,
    cache_dir: &Path,
    prefix: &str,
) -> Result<(), CliError> {
    if !src_dir.exists() {
        return Ok(());
    }
    let entries = fs::read_dir(src_dir).map_err(|e| {
        CliError::OverlaySyncError(src_dir.display().to_string(), e.to_string())
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            CliError::OverlaySyncError(src_dir.display().to_string(), e.to_string())
        })?;
        let path = entry.path();
        if path
            .extension()
            .and_then(|s| s.to_str())
            .map_or(false, |ext| ext == "overlayfile")
        {
            let filename = path.file_name().unwrap().to_string_lossy();
            let dest = cache_dir.join(format!("{}__{}", prefix, filename));
            fs::copy(&path, &dest).map_err(|e| {
                CliError::OverlaySyncError(path.display().to_string(), e.to_string())
            })?;
        }
    }
    Ok(())
}

/// Parse a GitHub URL like `https://github.com/owner/repo` into (owner, repo).
fn parse_github_url(url: &str) -> Result<(String, String), CliError> {
    let url = url.trim_end_matches('/');
    // Remove .git suffix if present
    let url = url.strip_suffix(".git").unwrap_or(url);
    let parts: Vec<&str> = url.split('/').collect();
    // Expect: https://github.com/owner/repo => [..., "github.com", "owner", "repo"]
    if parts.len() < 2 {
        return Err(CliError::OverlaySyncError(
            url.to_string(),
            "invalid GitHub URL: expected https://github.com/owner/repo".to_string(),
        ));
    }
    let repo = parts[parts.len() - 1].to_string();
    let owner = parts[parts.len() - 2].to_string();
    Ok((owner, repo))
}

fn fetch_github_overlayfiles(url: &str, cache_dir: &Path) -> Result<(), CliError> {
    let (owner, repo) = parse_github_url(url)?;

    let client = reqwest::blocking::Client::builder()
        .user_agent("oca-bin")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| {
            CliError::OverlaySyncError(url.to_string(), format!("http client error: {}", e))
        })?;

    // Fetch the repo tree recursively
    let tree_url = format!(
        "https://api.github.com/repos/{}/{}/git/trees/main?recursive=1",
        owner, repo
    );
    let response = client.get(&tree_url).send().map_err(|e| {
        CliError::OverlaySyncError(url.to_string(), format!("failed to fetch tree: {}", e))
    })?;

    if !response.status().is_success() {
        return Err(CliError::OverlaySyncError(
            url.to_string(),
            format!("GitHub API returned {}", response.status()),
        ));
    }

    let tree: serde_json::Value = response.json().map_err(|e| {
        CliError::OverlaySyncError(url.to_string(), format!("failed to parse response: {}", e))
    })?;

    let entries = tree["tree"]
        .as_array()
        .ok_or_else(|| {
            CliError::OverlaySyncError(url.to_string(), "unexpected API response format".to_string())
        })?;

    let overlayfiles: Vec<&str> = entries
        .iter()
        .filter_map(|entry| {
            let path = entry["path"].as_str()?;
            if path.ends_with(".overlayfile") && entry["type"].as_str() == Some("blob") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    info!(
        "Found {} overlayfile(s) in {}/{}",
        overlayfiles.len(),
        owner,
        repo
    );

    for file_path in overlayfiles {
        let raw_url = format!(
            "https://raw.githubusercontent.com/{}/{}/main/{}",
            owner, repo, file_path
        );
        let content = client.get(&raw_url).send().map_err(|e| {
            CliError::OverlaySyncError(
                file_path.to_string(),
                format!("failed to download: {}", e),
            )
        })?;

        if !content.status().is_success() {
            eprintln!(
                "Warning: failed to download {}: HTTP {}",
                file_path,
                content.status()
            );
            continue;
        }

        let body = content.text().map_err(|e| {
            CliError::OverlaySyncError(file_path.to_string(), format!("failed to read body: {}", e))
        })?;

        // Flatten the path into a safe filename: owner__repo__path_to_file.overlayfile
        let safe_name = file_path.replace('/', "__");
        let dest = cache_dir.join(format!("{}__{}_{}", owner, repo, safe_name));
        fs::write(&dest, body).map_err(|e| {
            CliError::OverlaySyncError(dest.display().to_string(), e.to_string())
        })?;
    }

    Ok(())
}
