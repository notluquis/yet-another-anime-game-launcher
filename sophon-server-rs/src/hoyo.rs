/// HoYoverse API constants and data-fetching helpers.
///
/// Mirrors the Python `retrieve_API_keys` / `make_getBuild_url` / `get_getBuild_json` flow,
/// but fully async and without on-disk caching (the server is ephemeral per-task).
use anyhow::{anyhow, bail, Result};
use serde_json::Value;

// ── Launcher / game IDs ──────────────────────────────────────────────────────

pub struct HoyoIds {
    pub base_hyp_url: &'static str,  // getGameBranches host
    pub launcher_id: &'static str,
    pub game_id: &'static str,
    pub get_build_host: &'static str, // getBuild host (install)
    pub get_patch_host: &'static str, // getPatchBuild host (update)
}

pub fn ids_for(rel_type: &str, game_type: &str) -> Result<HoyoIds> {
    match (rel_type, game_type) {
        ("os", "hk4e") => Ok(HoyoIds {
            base_hyp_url:    "https://sg-hyp-api.hoyoverse.com/hyp/hyp-connect/api",
            launcher_id:     "VYTpXlbWo8",
            game_id:         "gopR6Cufr3",
            get_build_host:  "https://sg-public-api.hoyoverse.com",
            get_patch_host:  "https://sg-downloader-api.hoyoverse.com",
        }),
        ("os", "nap") => Ok(HoyoIds {
            base_hyp_url:    "https://sg-hyp-api.hoyoverse.com/hyp/hyp-connect/api",
            launcher_id:     "VYTpXlbWo8",
            game_id:         "U5hbdsT9W7",
            get_build_host:  "https://sg-public-api.hoyoverse.com",
            get_patch_host:  "https://sg-downloader-api.hoyoverse.com",
        }),
        ("os", "hkrpg") => Ok(HoyoIds {
            base_hyp_url:    "https://sg-hyp-api.hoyoverse.com/hyp/hyp-connect/api",
            launcher_id:     "VYTpXlbWo8",
            game_id:         "4ziysqXOQ8",
            get_build_host:  "https://sg-public-api.hoyoverse.com",
            get_patch_host:  "https://sg-downloader-api.hoyoverse.com",
        }),
        ("cn", "hk4e") => Ok(HoyoIds {
            base_hyp_url:    "https://hyp-api.mihoyo.com/hyp/hyp-connect/api",
            launcher_id:     "jGHBHlcOq1",
            game_id:         "1Z8W5NHUQb",
            get_build_host:  "https://api-takumi.mihoyo.com",
            get_patch_host:  "https://api-takumi.mihoyo.com",
        }),
        ("cn", "nap") => Ok(HoyoIds {
            base_hyp_url:    "https://hyp-api.mihoyo.com/hyp/hyp-connect/api",
            launcher_id:     "jGHBHlcOq1",
            game_id:         "x6znKlJ0xK",
            get_build_host:  "https://api-takumi.mihoyo.com",
            get_patch_host:  "https://api-takumi.mihoyo.com",
        }),
        ("cn", "hkrpg") => Ok(HoyoIds {
            base_hyp_url:    "https://hyp-api.mihoyo.com/hyp/hyp-connect/api",
            launcher_id:     "jGHBHlcOq1",
            game_id:         "64kMb5iAWu",
            get_build_host:  "https://api-takumi.mihoyo.com",
            get_patch_host:  "https://api-takumi.mihoyo.com",
        }),
        ("bb", "hk4e") => Ok(HoyoIds {
            base_hyp_url:    "https://hyp-api.mihoyo.com/hyp/hyp-connect/api",
            launcher_id:     "umfgRO5gh5",
            game_id:         "T2S0Gz4Dr2",
            get_build_host:  "https://api-takumi.mihoyo.com",
            get_patch_host:  "https://api-takumi.mihoyo.com",
        }),
        _ => bail!("Unsupported rel_type={rel_type} game_type={game_type}"),
    }
}

// ── Branch info returned by getGameBranches ──────────────────────────────────

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub package_id: String,
    pub password: String,
    pub branch: String,
    pub tag: String,
    pub diff_tags: Vec<String>,
}

/// Fetch `getGameBranches` and extract the `main` or `pre_download` branch.
pub async fn get_branch(
    http: &reqwest::Client,
    ids: &HoyoIds,
    predownload: bool,
) -> Result<BranchInfo> {
    let url = format!(
        "{}/getGameBranches?game_ids[]={}&launcher_id={}",
        ids.base_hyp_url, ids.game_id, ids.launcher_id
    );

    let js: Value = http.get(&url).send().await?.json().await?;

    let retcode = js["retcode"].as_i64().unwrap_or(-1);
    if retcode != 0 {
        bail!("getGameBranches failed: retcode={} msg={}", retcode, js["message"]);
    }

    let branches = &js["data"]["game_branches"][0];
    let branch_key = if predownload { "pre_download" } else { "main" };
    let b = &branches[branch_key];

    if b.is_null() {
        bail!("Branch '{branch_key}' not available (pre-download may not be open yet)");
    }

    // diff_tags: list of versions from which a diff update is available
    let diff_tags: Vec<String> = b["diff_tags"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    Ok(BranchInfo {
        package_id: b["package_id"].as_str().ok_or_else(|| anyhow!("missing package_id"))?.to_string(),
        password:   b["password"].as_str().ok_or_else(|| anyhow!("missing password"))?.to_string(),
        branch:     b["branch"].as_str().ok_or_else(|| anyhow!("missing branch"))?.to_string(),
        tag:        b["tag"].as_str().ok_or_else(|| anyhow!("missing tag"))?.to_string(),
        diff_tags,
    })
}

// ── getBuild / getPatchBuild ─────────────────────────────────────────────────

/// Fetch and parse the `getBuild` JSON (for chunk-based install/repair).
pub async fn get_build(
    http: &reqwest::Client,
    ids: &HoyoIds,
    branch: &BranchInfo,
) -> Result<Value> {
    let url = format!(
        "{}/downloader/sophon_chunk/api/getBuild?branch={}&package_id={}&password={}",
        ids.get_build_host, branch.branch, branch.package_id, branch.password
    );
    let js: Value = http.get(&url).send().await?.json().await?;
    let retcode = js["retcode"].as_i64().unwrap_or(-1);
    if retcode != 0 {
        bail!("getBuild failed: retcode={} msg={}", retcode, js["message"]);
    }
    Ok(js["data"].clone())
}

/// Fetch and parse the `getPatchBuild` JSON (for ldiff-based update).
/// This is a POST with an empty body (the Python code passes `[]`).
pub async fn get_patch_build(
    http: &reqwest::Client,
    ids: &HoyoIds,
    branch: &BranchInfo,
) -> Result<Value> {
    let url = format!(
        "{}/downloader/sophon_chunk/api/getPatchBuild?branch={}&package_id={}&password={}",
        ids.get_patch_host, branch.branch, branch.package_id, branch.password
    );
    let js: Value = http
        .post(&url)
        .header("Content-Length", "0")
        .send()
        .await?
        .json()
        .await?;
    let retcode = js["retcode"].as_i64().unwrap_or(-1);
    if retcode != 0 {
        bail!("getPatchBuild failed: retcode={} msg={}", retcode, js["message"]);
    }
    Ok(js["data"].clone())
}

// ── Category selection ────────────────────────────────────────────────────────

/// Find the category entry (e.g. "game") inside a getBuild/getPatchBuild `data` value.
pub fn select_category<'a>(build_data: &'a Value, cat_name: &str) -> Result<&'a Value> {
    let manifests = build_data["manifests"]
        .as_array()
        .ok_or_else(|| anyhow!("build data has no 'manifests' array"))?;

    // Exact match first
    for m in manifests.iter() {
        if m["matching_field"].as_str() == Some(cat_name) {
            return Ok(m);
        }
    }
    // Fuzzy match (contains)
    let mut found: Option<&Value> = None;
    for m in manifests.iter() {
        if m["matching_field"]
            .as_str()
            .map(|f| f.contains(cat_name))
            .unwrap_or(false)
        {
            if found.is_some() {
                bail!("Ambiguous category '{cat_name}'");
            }
            found = Some(m);
        }
    }
    found.ok_or_else(|| anyhow!("Category '{cat_name}' not found in build manifests"))
}
