/// Resumable chunk download and file assembly.
///
/// Each "chunk" in the Sophon manifest is a separately downloadable piece of a
/// game file, stored compressed (zstd) on HoYo's CDN.  This module:
///
/// 1. Downloads the compressed chunk directly into memory (no tempfile).
/// 2. Decompresses it in memory.
/// 3. Writes the raw bytes to the assembled game file at the correct `offset`.
///
/// After all chunks have been written, the caller validates the file's MD5 and
/// moves it from `tempdir` into the game directory.
use anyhow::{bail, Context, Result};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use tokio::io::AsyncWriteExt;

/// Timings returned by [`write_chunk`] for bottleneck diagnosis.
pub struct ChunkWriteResult {
    /// Uncompressed bytes written to the assembled file.
    pub written: usize,
    /// Time spent downloading the compressed chunk from the CDN (milliseconds).
    pub download_ms: u64,
    /// Time spent decompressing + writing to disk (milliseconds).
    pub write_ms: u64,
}

/// Download `url` to `dst`, resuming from the current file size if it already
/// exists.  Handles HTTP 416 (already complete) gracefully.
pub async fn download_resumable(
    http: &reqwest::Client,
    url: &str,
    dst: &Path,
    expected_size: u64,
) -> Result<()> {
    let current_size = tokio::fs::metadata(dst).await.map(|m| m.len()).unwrap_or(0);

    if current_size == expected_size {
        return Ok(());
    }
    if current_size > expected_size {
        // Corrupted partial download – remove and restart.
        tracing::warn!("Removing oversized partial file {}", dst.display());
        tokio::fs::remove_file(dst).await.ok();
    }

    let current_size = tokio::fs::metadata(dst).await.map(|m| m.len()).unwrap_or(0);

    let response = if current_size > 0 {
        let range = format!("bytes={}-", current_size);
        http.get(url).header("Range", range).send().await?
    } else {
        http.get(url).send().await?
    };

    match response.status().as_u16() {
        416 => {
            // Out of range: the file we have is already complete.
            return Ok(());
        }
        200 | 206 => {}
        code => bail!("Unexpected HTTP {code} for {url}"),
    }

    // Stream response body to disk.
    use futures_util::StreamExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dst)
        .await
        .with_context(|| format!("open {}", dst.display()))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        file.write_all(&bytes).await?;
    }

    Ok(())
}

/// Download a compressed chunk directly into memory, decompress it, and write
/// at the given byte `offset` inside `assembled_file`.
///
/// Unlike the previous implementation this never writes the compressed bytes to
/// disk, saving two I/O operations per chunk (write compressed + read
/// compressed + delete → gone).  The only disk operation is the final
/// seek-and-write of the decompressed payload.
///
/// Returns a [`ChunkWriteResult`] with byte count and per-phase timing.
pub async fn write_chunk(
    http: &reqwest::Client,
    chunk_url_prefix: &str,
    chunk_id: &str,
    compressed_size: u32,
    offset: u64,
    assembled_file: &Path,
) -> Result<ChunkWriteResult> {
    let url = format!("{}/{}", chunk_url_prefix, chunk_id);

    // ── Download compressed bytes directly into memory ───────────────────────
    let t_dl = std::time::Instant::now();
    let response = http
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;

    match response.status().as_u16() {
        200 | 206 => {}
        code => bail!("Unexpected HTTP {code} for {url}"),
    }

    let compressed_bytes = response
        .bytes()
        .await
        .with_context(|| format!("read body {url}"))?;

    if compressed_bytes.len() != compressed_size as usize {
        bail!(
            "chunk {chunk_id}: expected {} bytes, got {}",
            compressed_size,
            compressed_bytes.len()
        );
    }
    let download_ms = t_dl.elapsed().as_millis() as u64;

    // ── Decompress + write on the blocking thread pool ───────────────────────
    let assembled_file2 = assembled_file.to_path_buf();
    let (written, write_ms) = tokio::task::spawn_blocking(move || -> Result<(usize, u64)> {
        let t_wr = std::time::Instant::now();

        let decompressed = zstd::decode_all(compressed_bytes.as_ref())
            .context("zstd decompress chunk")?;

        let n = decompressed.len();
        let mut fh = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&assembled_file2)
            .with_context(|| format!("open assembled {}", assembled_file2.display()))?;
        fh.seek(SeekFrom::Start(offset))?;
        fh.write_all(&decompressed)?;

        Ok((n, t_wr.elapsed().as_millis() as u64))
    })
    .await??;

    Ok(ChunkWriteResult {
        written,
        download_ms,
        write_ms,
    })
}

/// Verify the MD5 of a file.
pub async fn verify_md5(path: &Path, expected: &str) -> Result<bool> {
    let data = tokio::fs::read(path).await?;
    let digest = tokio::task::spawn_blocking(move || {
        format!("{:x}", md5::compute(&data))
    })
    .await?;
    Ok(digest == expected)
}
