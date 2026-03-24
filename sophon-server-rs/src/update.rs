/// Incremental update via ldiff (hpatchz) + new-file chunk downloads.
use anyhow::{Context, Result};
use futures_util::{stream, StreamExt, TryStreamExt};
use serde_json::json;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::downloader::{download_resumable, write_chunk};
use crate::hpatchz::apply_patch;
use crate::hoyo::{get_branch, get_build, get_patch_build, ids_for, select_category};
use crate::manifest::{
    download_manifest, path_safety_check, update_config_version, DiffManifest, Manifest,
};
use crate::models::UpdateParams;
use crate::repair::detect_rel_type;
use crate::state::TaskEntry;

const FILE_CONCURRENCY: usize = 4;

pub async fn perform_update(
    http: reqwest::Client,
    params: UpdateParams,
    entry: Arc<TaskEntry>,
) -> Result<()> {
    let gamedir = params.gamedir.clone();
    let tempdir = params.tempdir.clone();
    let ldiff_dir = gamedir.join("ldiff");

    tokio::fs::create_dir_all(&tempdir).await?;
    tokio::fs::create_dir_all(&ldiff_dir).await?;

    // ── Detect installed version ────────────────────────────────────────────
    let rel_type = detect_rel_type(&params.game_type, &gamedir).await?;
    let installed_ver = detect_installed_version(&params.game_type, &gamedir).await?;

    // ── Fetch HoYo API ──────────────────────────────────────────────────────
    let ids = ids_for(&rel_type, &params.game_type)?;
    let branch = get_branch(&http, &ids, params.predownload).await?;

    if !params.predownload && installed_ver == branch.tag {
        anyhow::bail!("Game is already up-to-date ({})", installed_ver);
    }

    // Chunks build: for new files
    let chunks_build = get_build(&http, &ids, &branch).await?;
    // Patch build: for ldiff patches (same branch)
    let patch_build = get_patch_build(&http, &ids, &branch).await?;

    let target_ver = chunks_build["tag"].as_str().unwrap_or("0.0.0").to_string();

    // ── Download manifests ──────────────────────────────────────────────────
    let chunks_cat = select_category(&chunks_build, "game")?;
    let chunks_manifest: Manifest = download_manifest(&http, chunks_cat).await?;

    let diffs_cat = select_category(&patch_build, "game")?;
    let diffs_manifest: DiffManifest = download_manifest(&http, diffs_cat).await?;

    let chunk_url_prefix = chunks_cat["chunk_download"]["url_prefix"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chunk_download.url_prefix missing"))?
        .to_string();

    let diff_url_prefix = diffs_cat["diff_download"]["url_prefix"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("diff_download.url_prefix missing"))?
        .to_string();

    // ── 1. Delete old files ─────────────────────────────────────────────────
    if !params.predownload {
        let delete_list: Vec<_> = diffs_manifest
            .files_delete
            .iter()
            .filter(|d| d.key == installed_ver)
            .flat_map(|d| d.info.as_ref().map(|i| i.list.clone()).unwrap_or_default())
            .collect();

        send(&entry, "delete_file_summary", json!({ "total_files": delete_list.len() }));

        for v in &delete_list {
            if entry.is_cancelled() { send_error(&entry, "cancelled"); return Ok(()); }
            path_safety_check(&v.filename)?;
            let path = gamedir.join(&v.filename); // gamedir is owned
            if path.exists() {
                tokio::fs::remove_file(&path).await.ok();
                send(&entry, "delete_file", json!({ "filename": v.filename }));
            }
        }
    }

    // ── 2. Apply ldiff patches ─────────────────────────────────────────────
    let mut new_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Find which files in the chunks manifest are NOT in the diff manifest
    // (they are brand-new in this version).
    let diff_filenames: std::collections::HashSet<&str> = diffs_manifest
        .files
        .iter()
        .map(|f| f.filename.as_str())
        .collect();

    for file_info in &chunks_manifest.files {
        if !diff_filenames.contains(file_info.filename.as_str()) && file_info.flags != 64 {
            new_files.insert(file_info.filename.clone());
        }
    }

    // Compute total ldiff download size
    let ldiff_total_size: u64 = diffs_manifest
        .files
        .iter()
        .filter_map(|f| f.patches.iter().find(|p| p.key == installed_ver).map(|p| p.info.as_ref().map(|i| i.patch_size as u64).unwrap_or(0)))
        .sum();

    send(&entry, "ldiff_download_summary", json!({
        "ldiff_file_count": diffs_manifest.files.len(),
        "ldiff_total_size": ldiff_total_size,
    }));

    let mut ldiff_downloaded: u64 = 0;
    let mut speed_ema: f64 = 0.0;

    for diff_file in &diffs_manifest.files {
        if entry.is_cancelled() { send_error(&entry, "cancelled"); return Ok(()); }

        path_safety_check(&diff_file.filename)?;

        // Find the patch for our installed version
        let patch = match diff_file.patches.iter().find(|p| p.key == installed_ver) {
            Some(p) => p,
            None => {
                // File unchanged relative to installed_ver → no patch needed
                send(&entry, "ldiff_download_skipped", json!({
                    "filename": diff_file.filename,
                    "reason": "no_patch_for_version",
                }));
                continue;
            }
        };

        let info = match patch.info.as_ref() {
            Some(i) => i,
            None => continue,
        };

        send(&entry, "ldiff_download_start", json!({ "filename": diff_file.filename }));

        // Download the patch file
        let patch_file_path = ldiff_dir.join(&info.patch_name);
        let patch_url = format!("{}/{}", diff_url_prefix, info.patch_id);
        let t0 = std::time::Instant::now();
        download_resumable(&http, &patch_url, &patch_file_path, info.patch_size as u64).await?;
        let elapsed = t0.elapsed().as_secs_f64().max(0.001);
        let instant_speed = info.patch_size as f64 / elapsed;
        speed_ema = if speed_ema == 0.0 { instant_speed } else { 0.3 * instant_speed + 0.7 * speed_ema };

        ldiff_downloaded += info.patch_size as u64;
        send(&entry, "ldiff_download_complete", json!({
            "filename": diff_file.filename,
            "file_size": info.patch_size,
            "overall_progress": {
                "downloaded_size": ldiff_downloaded,
                "total_size": ldiff_total_size,
                "overall_percent": ldiff_downloaded as f64 / ldiff_total_size.max(1) as f64 * 100.0,
                "download_speed": speed_ema as u64,
            },
        }));

        if params.predownload {
            // Pre-download mode: only download, don't apply
            continue;
        }

        // Apply patch: old → new
        send(&entry, "ldiff_patch_start", json!({ "filename": diff_file.filename }));

        let src_file = gamedir.join(&info.original_name);
        let tmp_patched = tempdir.join(
            std::path::Path::new(&diff_file.filename)
                .file_name()
                .unwrap_or_default(),
        );

        apply_patch(&src_file, &patch_file_path, info.patch_offset, info.patch_length, &tmp_patched)
            .await
            .with_context(|| format!("hpatchz on {}", diff_file.filename))?;

        // Move patched file to gamedir
        let dest = gamedir.join(&diff_file.filename);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if tokio::fs::rename(&tmp_patched, &dest).await.is_err() {
            tokio::fs::copy(&tmp_patched, &dest).await?;
            tokio::fs::remove_file(&tmp_patched).await.ok();
        }
        tokio::fs::remove_file(&patch_file_path).await.ok();

        send(&entry, "ldiff_patch_complete", json!({ "filename": diff_file.filename }));
    }

    if params.predownload {
        return Ok(());
    }

    // ── 3. Download brand-new files ─────────────────────────────────────────
    let new_file_list: Vec<_> = chunks_manifest
        .files
        .iter()
        .filter(|f| new_files.contains(&f.filename))
        .collect();

    let new_total_size: u64 = new_file_list
        .iter()
        .flat_map(|f| f.chunks.iter())
        .map(|c| c.compressed_size as u64)
        .sum();

    send(&entry, "download_summary", json!({
        "game_version": target_ver,
        "download_size": new_total_size,
        "download_file_count": new_file_list.len(),
        "download_categories": ["game"],
    }));

    let downloaded_bytes = Arc::new(AtomicU64::new(0));
    let session_start = std::time::Instant::now();
    let chunk_url_prefix = Arc::<str>::from(chunk_url_prefix.as_str());

    let dl_result = stream::iter(new_file_list.into_iter().cloned())
        .map(Ok::<_, anyhow::Error>)
        .try_for_each_concurrent(FILE_CONCURRENCY, |file_info| {
            let http = http.clone();
            let entry = entry.clone();
            let db = Arc::clone(&downloaded_bytes);
            let prefix = Arc::clone(&chunk_url_prefix);
            let tempdir = tempdir.clone();
            let gamedir = gamedir.clone();

            async move {
                if entry.is_cancelled() {
                    return Err(anyhow::anyhow!("cancelled"));
                }

                path_safety_check(&file_info.filename)?;
                let dest = gamedir.join(&file_info.filename);

                let tmp_assembled = tempdir.join("partial").join(&file_info.filename);
                if let Some(p) = tmp_assembled.parent() {
                    tokio::fs::create_dir_all(p).await?;
                }

                send(&entry, "file_download_start", json!({ "filename": file_info.filename }));

                let already_done = tokio::fs::metadata(&tmp_assembled)
                    .await.map(|m| m.len() == file_info.size as u64).unwrap_or(false);

                if !already_done {
                    {
                        let fh = std::fs::File::create(&tmp_assembled)?;
                        fh.set_len(file_info.size as u64)?;
                    }
                    for (i, chunk) in file_info.chunks.iter().enumerate() {
                        if entry.is_cancelled() {
                            return Err(anyhow::anyhow!("cancelled"));
                        }

                        let result = write_chunk(
                            &http, &prefix, &chunk.chunk_id,
                            chunk.compressed_size, chunk.offset, &tmp_assembled,
                        ).await?;

                        let cur = db.fetch_add(chunk.compressed_size as u64, Ordering::Relaxed)
                            + chunk.compressed_size as u64;
                        let elapsed = session_start.elapsed().as_secs_f64().max(0.001);
                        let download_speed = cur as f64 / elapsed;
                        let write_speed = if result.write_ms > 0 {
                            result.written as f64 / (result.write_ms as f64 / 1000.0)
                        } else {
                            result.written as f64 / 0.001
                        };

                        send(&entry, "chunk_progress", json!({
                            "filename": file_info.filename,
                            "total_chunks": file_info.chunks.len(),
                            "current_chunk": i + 1,
                            "progress_percent": (chunk.offset + result.written as u64) as f64 / file_info.size.max(1) as f64 * 100.0,
                            "current_byte": chunk.offset + result.written as u64,
                            "total_bytes": file_info.size,
                            "chunk_size": chunk.compressed_size,
                            "download_ms": result.download_ms,
                            "write_ms": result.write_ms,
                            "overall_progress": {
                                "downloaded_size": cur,
                                "total_size": new_total_size,
                                "overall_percent": cur as f64 / new_total_size.max(1) as f64 * 100.0,
                                "download_speed": download_speed as u64,
                                "write_speed": write_speed as u64,
                            },
                        }));
                    }
                }

                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                if tokio::fs::rename(&tmp_assembled, &dest).await.is_err() {
                    tokio::fs::copy(&tmp_assembled, &dest).await?;
                    tokio::fs::remove_file(&tmp_assembled).await.ok();
                }

                send(&entry, "file_download_complete", json!({
                    "filename": file_info.filename,
                    "file_size": file_info.size,
                }));

                Ok(())
            }
        })
        .await;

    match dl_result {
        Ok(()) => {}
        Err(e) if e.to_string() == "cancelled" => {
            send_error(&entry, "cancelled");
            return Ok(());
        }
        Err(e) => return Err(e),
    }

    // ── 4. Update config.ini ────────────────────────────────────────────────
    let config_path = gamedir.join("config.ini"); // gamedir is owned
    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await?;
        let updated = update_config_version(&content, &target_ver);
        tokio::fs::write(&config_path, updated).await?;
    }

    // ── 5. Cleanup ldiff dir ────────────────────────────────────────────────
    if ldiff_dir.exists() {
        tokio::fs::remove_dir_all(&ldiff_dir).await.ok();
    }

    Ok(())
}

/// Read the installed game version from globalgamemanagers or config.ini.
async fn detect_installed_version(game_type: &str, gamedir: &std::path::Path) -> Result<String> {
    // Try config.ini first (most reliable after a clean update)
    let config_path = gamedir.join("config.ini");
    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await?;
        if let Some(ver) = crate::manifest::read_config_version(&content) {
            return Ok(ver);
        }
    }

    // Fall back to globalgamemanagers binary scan (hk4e pattern)
    if game_type == "hk4e" {
        let mut dir = tokio::fs::read_dir(gamedir).await?;
        while let Some(ent) = dir.next_entry().await? {
            let name = ent.file_name();
            if name.to_string_lossy().ends_with("_Data") {
                let ggm = ent.path().join("globalgamemanagers");
                if ggm.exists() {
                    let data = tokio::fs::read(&ggm).await?;
                    let re = regex::bytes::Regex::new(r"\x00(\d+\.\d+\.\d+)_\d+_\d+\x00").unwrap();
                    if let Some(cap) = re.captures(&data) {
                        return Ok(String::from_utf8_lossy(&cap[1]).to_string());
                    }
                }
            }
        }
    }

    anyhow::bail!("Cannot determine installed version for {game_type} in {}", gamedir.display())
}

fn send(entry: &Arc<TaskEntry>, kind: &str, mut payload: serde_json::Value) {
    payload["type"] = serde_json::Value::String(kind.to_string());
    payload["task_id"] = serde_json::Value::String(
        entry.status.lock().unwrap().task_id.clone(),
    );
    let s = serde_json::to_string(&payload).unwrap_or_default();
    let _ = entry.tx.send(s);
}

fn send_error(entry: &Arc<TaskEntry>, error: &str) {
    send(entry, "job_error", json!({ "error": error }));
}
