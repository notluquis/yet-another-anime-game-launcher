/// Protobuf types and manifest download helpers.
use anyhow::Result;
use bytes::Bytes;
use prost::Message;

// ── prost-generated types ──────────────────────────────────────────────────

pub mod manifest_proto {
    include!(concat!(env!("OUT_DIR"), "/manifest.rs"));
}

pub mod manifest_ldiff_proto {
    include!(concat!(env!("OUT_DIR"), "/manifest_ldiff.rs"));
}

pub use manifest_proto::*;
pub use manifest_ldiff_proto::*;

// ── Manifest download ──────────────────────────────────────────────────────

/// Download, zstd-decompress, and decode a protobuf manifest.
/// `T` is either `Manifest` (chunks) or `DiffManifest` (ldiff).
pub async fn download_manifest<T: Message + Default>(
    http: &reqwest::Client,
    category: &serde_json::Value,
) -> Result<T> {
    let manifest_id = category["manifest"]["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("manifest.id missing"))?;
    let url_prefix = category["manifest_download"]["url_prefix"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("manifest_download.url_prefix missing"))?;

    let url = format!("{}/{}", url_prefix, manifest_id);
    tracing::debug!("Downloading manifest {url}");

    let compressed: Bytes = http.get(&url).send().await?.bytes().await?;

    // Decompression is CPU-bound; run on blocking thread pool.
    let decompressed = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let decoded = zstd::decode_all(compressed.as_ref())?;
        Ok(decoded)
    })
    .await??;

    Ok(T::decode(decompressed.as_slice())?)
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Reject paths with `..` or leading `/` to prevent directory traversal.
pub fn path_safety_check(filename: &str) -> Result<()> {
    if filename.contains("..") || filename.starts_with('/') {
        anyhow::bail!("Unsafe path rejected: {filename}");
    }
    Ok(())
}

/// Parse `game_version=X.Y.Z` from config.ini content.
pub fn read_config_version(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.strip_prefix("game_version=")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

/// Swap the `game_version=` line in config.ini content.
pub fn update_config_version(content: &str, new_ver: &str) -> String {
    let re = regex::Regex::new(r"game_version=\d+\.\d+\.\d+").unwrap();
    re.replace(content, format!("game_version={new_ver}")).into_owned()
}
