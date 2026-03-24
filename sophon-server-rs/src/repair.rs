/// Repair operation: verify files against the manifest and re-download damaged ones.
use anyhow::Result;
use futures_util::{stream, StreamExt, TryStreamExt};
use serde_json::json;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::downloader::{verify_md5, write_chunk};
use crate::hoyo::{get_branch, get_build, ids_for, select_category};
use crate::manifest::{download_manifest, path_safety_check, Manifest};
use crate::models::RepairParams;
use crate::state::TaskEntry;

const FILE_CONCURRENCY: usize = 4;

pub async fn perform_repair(
    http: reqwest::Client,
    params: RepairParams,
    entry: Arc<TaskEntry>,
) -> Result<()> {
    let gamedir = params.gamedir.clone();
    let tempdir = params.tempdir.clone();

    tokio::fs::create_dir_all(&tempdir).await?;

    // Detect rel_type from config.ini or game binary
    let rel_type = detect_rel_type(&params.game_type, &gamedir).await?;

    // ── Fetch API ──────────────────────────────────────────────────────────
    let ids = ids_for(&rel_type, &params.game_type)?;
    let branch = get_branch(&http, &ids, false).await?;
    let build_data = get_build(&http, &ids, &branch).await?;

    let tag = build_data["tag"].as_str().unwrap_or("0.0.0").to_string();

    // ── Download manifest ──────────────────────────────────────────────────
    let category = select_category(&build_data, "game")?;
    let manifest: Manifest = download_manifest(&http, category).await?;

    let chunk_url_prefix = category["chunk_download"]["url_prefix"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chunk_download.url_prefix missing"))?
        .to_string();

    let total_files = manifest.files.len();
    let reliable = params.repair_mode == "reliable";

    send(&entry, "repair_summary", json!({
        "repair_mode": params.repair_mode,
        "total_files": total_files,
    }));

    // ── Verify phase ────────────────────────────────────────────────────────
    let mut files_to_repair: Vec<usize> = Vec::new(); // indices into manifest.files

    for (idx, file_info) in manifest.files.iter().enumerate() {
        if entry.is_cancelled() {
            send_error(&entry, "cancelled");
            return Ok(());
        }
        if file_info.flags == 64 {
            continue; // directory
        }

        path_safety_check(&file_info.filename)?;
        let dest = gamedir.join(&file_info.filename); // uses owned gamedir above

        let reason = match tokio::fs::metadata(&dest).await {
            Err(_) => Some("missing".to_string()),
            Ok(meta) => {
                if meta.len() != file_info.size as u64 {
                    Some(format!(
                        "size mismatch. is={}, should={}",
                        meta.len(),
                        file_info.size
                    ))
                } else if reliable {
                    if !verify_md5(&dest, &file_info.md5).await.unwrap_or(false) {
                        Some("md5 mismatch".to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        let needs_repair = reason.is_some();
        if idx % 10 == 0 {
            send(&entry, "check_file", json!({
                "filename": file_info.filename,
                "requires_repair": needs_repair,
                "reason": reason.clone().unwrap_or_default(),
                "overall_progress": {
                    "total_files": total_files,
                    "checked_files": idx + 1,
                    "overall_percent": (idx + 1) as f64 / total_files as f64 * 100.0,
                },
            }));
        }

        if needs_repair {
            files_to_repair.push(idx);
        }
    }

    // ── Download damaged files ──────────────────────────────────────────────
    let total_size: u64 = files_to_repair.iter()
        .flat_map(|&i| manifest.files[i].chunks.iter())
        .map(|c| c.compressed_size as u64)
        .sum();

    send(&entry, "download_summary", json!({
        "game_version": tag,
        "download_size": total_size,
        "download_file_count": files_to_repair.len(),
        "download_categories": ["game"],
    }));

    // Collect the actual FileInfo objects for files that need repair
    let repair_files: Vec<_> = files_to_repair.iter()
        .map(|&i| manifest.files[i].clone())
        .collect();

    let downloaded_bytes = Arc::new(AtomicU64::new(0));
    let session_start = std::time::Instant::now();
    let chunk_url_prefix = Arc::<str>::from(chunk_url_prefix.as_str());

    let dl_result = stream::iter(repair_files.iter().cloned())
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

                let dest = gamedir.join(&file_info.filename);
                let tmp_assembled = tempdir.join("partial").join(&file_info.filename);
                if let Some(p) = tmp_assembled.parent() {
                    tokio::fs::create_dir_all(p).await?;
                }

                send(&entry, "file_download_start", json!({ "filename": file_info.filename }));

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
                            chunk.compressed_size, chunk.offset,
                            &tmp_assembled,
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
                                "total_size": total_size,
                                "overall_percent": cur as f64 / total_size.max(1) as f64 * 100.0,
                                "download_speed": download_speed as u64,
                                "write_speed": write_speed as u64,
                            },
                        }));
                }

                if !verify_md5(&tmp_assembled, &file_info.md5).await? {
                    tokio::fs::remove_file(&tmp_assembled).await.ok();
                    anyhow::bail!("MD5 mismatch after repair for: {}", file_info.filename);
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

    Ok(())
}

/// Detect release type from config.ini (for nap) or game executable (for hk4e).
pub async fn detect_rel_type(game_type: &str, gamedir: &std::path::Path) -> Result<String> {
    match game_type {
        "hk4e" => {
            if gamedir.join("GenshinImpact.exe").exists() {
                Ok("os".into())
            } else if gamedir.join("YuanShen.exe").exists() {
                // Bilibili has PCGameSDK.dll in *_Data/Plugins/
                let bb = async {
                    let mut dir = tokio::fs::read_dir(gamedir).await?;
                    while let Some(entry) = dir.next_entry().await? {
                        let name = entry.file_name();
                        let s = name.to_string_lossy();
                        if s.ends_with("_Data") {
                            if entry.path().join("Plugins/PCGameSDK.dll").exists() {
                                return Ok::<bool, anyhow::Error>(true);
                            }
                        }
                    }
                    Ok(false)
                }.await?;
                Ok(if bb { "bb" } else { "cn" }.into())
            } else {
                anyhow::bail!("Cannot detect hk4e release type: game executable not found in {}", gamedir.display())
            }
        }
        "nap" => {
            let config = tokio::fs::read_to_string(gamedir.join("config.ini")).await?;
            if config.contains("sub_channel=0") {
                Ok("os".into())
            } else if config.contains("sub_channel=1") {
                Ok("cn".into())
            } else {
                anyhow::bail!("Cannot detect nap release type from config.ini")
            }
        }
        other => anyhow::bail!("Unknown game_type: {other}"),
    }
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
