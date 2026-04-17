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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

/// Max HTTP retries for a single chunk download on transient failure
/// (network error, 5xx, partial body, decompress error).
const CHUNK_MAX_ATTEMPTS: u32 = 4;
/// Base backoff in milliseconds — actual delay is `BASE * 2^attempt`.
const CHUNK_BACKOFF_BASE_MS: u64 = 300;

/// Shared handle to an assembled output file. The lock is held only during the
/// seek-and-write of a single chunk, so parallel *tasks* (which each own their
/// own file) never contend; it exists because `spawn_blocking` requires the
/// inner closure to be `'static + Send`.
pub type AssembledFile = Arc<Mutex<std::fs::File>>;

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
/// at the given byte `offset` inside the caller-supplied `file`.
///
/// The caller is responsible for opening the assembled file once at task start
/// and pre-allocating its final size, avoiding a pair of `open()`/`close()`
/// syscalls per chunk (hundreds per file). The lock is held only for the
/// seek-and-write; serial inside a task, so no contention.
///
/// Retries transient failures up to [`CHUNK_MAX_ATTEMPTS`] times with
/// exponential backoff. The `cancel` flag is polled during the streamed
/// download so a cancelled task aborts within ~one TCP packet instead of
/// waiting for the whole chunk body.
///
/// Returns a [`ChunkWriteResult`] with byte count and per-phase timing.
pub async fn write_chunk(
    http: &reqwest::Client,
    chunk_url_prefix: &str,
    chunk_id: &str,
    compressed_size: u32,
    offset: u64,
    file: AssembledFile,
    cancel: Arc<AtomicBool>,
) -> Result<ChunkWriteResult> {
    let url = format!("{}/{}", chunk_url_prefix, chunk_id);

    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..CHUNK_MAX_ATTEMPTS {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }

        if attempt > 0 {
            let backoff = CHUNK_BACKOFF_BASE_MS * (1u64 << (attempt - 1));
            tracing::warn!(
                "chunk {chunk_id}: attempt {}/{} after error: {:#} (backoff {}ms)",
                attempt + 1,
                CHUNK_MAX_ATTEMPTS,
                last_err.as_ref().unwrap(),
                backoff,
            );
            tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
        }

        match try_write_chunk(http, &url, chunk_id, compressed_size, offset, &file, &cancel).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if e.to_string() == "cancelled" {
                    return Err(e);
                }
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("chunk {chunk_id}: exhausted retries")))
}

/// Single attempt — downloads the body streaming, decompresses, writes at
/// `offset`. Any error (network, body size mismatch, zstd, IO) is returned
/// so [`write_chunk`] can decide whether to retry.
async fn try_write_chunk(
    http: &reqwest::Client,
    url: &str,
    chunk_id: &str,
    compressed_size: u32,
    offset: u64,
    file: &AssembledFile,
    cancel: &Arc<AtomicBool>,
) -> Result<ChunkWriteResult> {
    use futures_util::StreamExt;

    let t_dl = std::time::Instant::now();
    let response = http
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;

    match response.status().as_u16() {
        200 | 206 => {}
        code => bail!("Unexpected HTTP {code} for {url}"),
    }

    let mut buf: Vec<u8> = Vec::with_capacity(compressed_size as usize);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let bytes = chunk.with_context(|| format!("stream body {url}"))?;
        buf.extend_from_slice(&bytes);
        if buf.len() > compressed_size as usize {
            bail!(
                "chunk {chunk_id}: overran expected size {}",
                compressed_size
            );
        }
    }

    if buf.len() != compressed_size as usize {
        bail!(
            "chunk {chunk_id}: expected {} bytes, got {}",
            compressed_size,
            buf.len()
        );
    }
    let download_ms = t_dl.elapsed().as_millis() as u64;

    if cancel.load(Ordering::Relaxed) {
        bail!("cancelled");
    }

    let file = Arc::clone(file);
    let (written, write_ms) = tokio::task::spawn_blocking(move || -> Result<(usize, u64)> {
        let t_wr = std::time::Instant::now();

        let decompressed = zstd::decode_all(buf.as_slice())
            .context("zstd decompress chunk")?;

        let n = decompressed.len();
        let mut fh = file
            .lock()
            .map_err(|_| anyhow::anyhow!("assembled file mutex poisoned"))?;
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

/// Open the assembled output file with pre-allocated size, or reuse an existing
/// partial (same length) for chunk-level resume.
pub fn open_assembled(path: &Path, size: u64, fresh: bool) -> Result<AssembledFile> {
    let file = if fresh {
        let f = std::fs::File::create(path)
            .with_context(|| format!("create assembled {}", path.display()))?;
        f.set_len(size)?;
        f
    } else {
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .with_context(|| format!("open assembled {}", path.display()))?
    };
    Ok(Arc::new(Mutex::new(file)))
}

/// Verify the MD5 of a file without loading it entirely in memory.
///
/// Streams the file through an md5 context in 1 MiB blocks on the blocking
/// thread pool, so the peak memory is bounded regardless of file size.
pub async fn verify_md5(path: &Path, expected: &str) -> Result<bool> {
    const BLOCK: usize = 1024 * 1024;
    let path = path.to_path_buf();
    let expected = expected.to_ascii_lowercase();
    tokio::task::spawn_blocking(move || -> Result<bool> {
        use std::io::Read;
        let mut file = std::fs::File::open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        let mut ctx = md5::Context::new();
        let mut buf = vec![0u8; BLOCK];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            ctx.consume(&buf[..n]);
        }
        let digest = format!("{:x}", ctx.compute());
        Ok(digest == expected)
    })
    .await?
}
