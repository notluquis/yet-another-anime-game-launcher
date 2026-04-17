/// Wrapper around the hpatchz binary for applying ldiff patches.
///
/// hpatchz usage: `hpatchz -f <old_file> <patch_file> <output_file>`
///
/// A single ldiff file may contain multiple concatenated patches, so we first
/// extract the relevant section (offset + length) to a temp file before running
/// hpatchz on it.
use anyhow::{bail, Context, Result};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Return the path to the hpatchz binary.  Searched in order:
/// 1. Sibling of this executable (`<exe_dir>/hpatchz`)
/// 2. `<exe_dir>/../hpatchz/hpatchz`
/// 3. `hpatchz` in PATH
pub fn hpatchz_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.parent().map(|p| p.join("hpatchz")).unwrap_or_default();
        if sibling.is_file() {
            return sibling;
        }
        let parent = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("hpatchz").join("hpatchz"))
            .unwrap_or_default();
        if parent.is_file() {
            return parent;
        }
    }
    PathBuf::from("hpatchz") // fall back to PATH
}

/// Apply a patch to `old_file`, producing `dst_file`.
///
/// The patch to apply lives at `[p_offset, p_offset + p_length)` inside
/// `patch_file`.  We extract that slice to a temporary file because hpatchz
/// does not support an offset parameter.
pub async fn apply_patch(
    old_file: &Path,
    patch_file: &Path,
    p_offset: i64,
    p_length: i64,
    dst_file: &Path,
    tempdir: &Path,
) -> Result<()> {
    let patch_file = patch_file.to_path_buf();
    let old_file = old_file.to_path_buf();
    let dst_file = dst_file.to_path_buf();
    let tempdir = tempdir.to_path_buf();
    let hpatchz = hpatchz_path();

    tokio::task::spawn_blocking(move || -> Result<()> {
        // Extract patch slice to a named temp file on the caller-supplied
        // tempdir (SSD), avoiding HDD thrash when the game lives on an
        // external/slow drive.
        let mut src = std::fs::File::open(&patch_file)
            .with_context(|| format!("open patch {}", patch_file.display()))?;
        src.seek(SeekFrom::Start(p_offset as u64))?;
        let mut patch_slice = vec![0u8; p_length as usize];
        src.read_exact(&mut patch_slice)
            .context("read patch slice")?;

        let mut tmp = tempfile::NamedTempFile::new_in(&tempdir)
            .with_context(|| format!("create temp patch file in {}", tempdir.display()))?;
        tmp.write_all(&patch_slice)?;
        tmp.flush()?;

        let output = std::process::Command::new(&hpatchz)
            .args(["-f", &old_file.to_string_lossy(), &tmp.path().to_string_lossy(), &dst_file.to_string_lossy()])
            .output()
            .with_context(|| format!("run hpatchz ({})", hpatchz.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "hpatchz exit {:?} on '{}': stderr=<{}> stdout=<{}>",
                output.status.code(),
                old_file.display(),
                stderr.trim(),
                stdout.trim(),
            );
        }
        Ok(())
    })
    .await?
}
