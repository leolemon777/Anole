//! Optional engine packs fetched on demand (spec E-04, DECISION-3 a).
//!
//! The installer stays slim; the Engines page offers curated packs that the
//! user explicitly downloads. Every archive is pinned by SHA-256 — a pack
//! whose hash constant is not yet pinned disables its own download button —
//! and the fetched archive goes through the same verified-install path as a
//! locally imported pack. Downloads are user-initiated and disclosed in
//! PRIVACY.md; this is the only outbound traffic the app itself initiates
//! besides the updater.

use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// A curated optional pack. `archive_sha256` is pinned at release time; an
/// empty hash means "announced but not yet published" and disables download.
pub struct OptionalPackSpec {
    pub pack_id: &'static str,
    pub engine_id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub archive_url: &'static str,
    pub archive_sha256: &'static str,
    pub size_bytes: u64,
}

/// The Document pack: `LibreOffice` (MPL-2.0, "substantially unmodified"
/// redistribution) enabling Office→PDF out of the box. Pinned when the
/// pack is attached to a release (DECISION-3 approved 2026-09-07).
pub const DOCUMENT_PACK: OptionalPackSpec = OptionalPackSpec {
    pack_id: "document",
    engine_id: "formatwright-document",
    display_name: "Document pack (LibreOffice)",
    description: "docx/xlsx/pptx → PDF without installing LibreOffice yourself",
    archive_url: "https://github.com/leolemon777/FormatWright/releases/download/v0.1.1/document-pack-windows-x86_64.zip",
    archive_sha256: "",
    size_bytes: 0,
};

pub const OPTIONAL_PACKS: &[OptionalPackSpec] = &[DOCUMENT_PACK];

pub fn optional_pack_by_id(pack_id: &str) -> Option<&'static OptionalPackSpec> {
    let index = OPTIONAL_PACKS
        .iter()
        .position(|spec| spec.pack_id == pack_id)?;
    OPTIONAL_PACKS.get(index)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionalPackView {
    pub pack_id: String,
    pub display_name: String,
    pub description: String,
    pub size_bytes: u64,
    /// False while the release hash is not pinned yet (button disabled).
    pub downloadable: bool,
    /// True when an activated registry entry already provides the engine.
    pub installed: bool,
}

pub fn optional_pack_views(registry_directory: &Path) -> Vec<OptionalPackView> {
    OPTIONAL_PACKS
        .iter()
        .map(|spec| OptionalPackView {
            pack_id: spec.pack_id.to_owned(),
            display_name: spec.display_name.to_owned(),
            description: spec.description.to_owned(),
            size_bytes: spec.size_bytes,
            downloadable: !spec.archive_sha256.is_empty() && spec.size_bytes > 0,
            installed: registry_directory
                .join(format!("{}.json", spec.engine_id))
                .is_file(),
        })
        .collect()
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("reading the downloaded archive failed: {error}"))?;
        if read == 0 {
            break;
        }
        hasher
            .write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Extracts a pack archive (a zip containing `manifest.json` at its root or
/// inside a single top-level directory) next to `manifest.json`'s parent.
fn extract_pack_archive(archive: &Path, destination: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(archive).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| format!("the downloaded pack is not a valid zip archive: {error}"))?;
    let mut manifest_candidates: Vec<String> = zip
        .file_names()
        .filter(|name| {
            let name = name.trim_end_matches('/');
            name.ends_with("manifest.json") && !name.split('/').any(|segment| segment == "..")
        })
        .map(str::to_owned)
        .collect();
    manifest_candidates.sort_by_key(|name| name.matches('/').count());
    let manifest_entry = manifest_candidates
        .first()
        .ok_or_else(|| "the pack archive contains no manifest.json".to_owned())?
        .clone();
    let strip_prefix = match manifest_entry.matches('/').count() {
        0 => None,
        1 => Some(
            manifest_entry
                .split_once('/')
                .map(|(directory, _)| directory.to_owned())
                .unwrap_or_default(),
        ),
        _ => return Err("the pack manifest must sit at the archive root".to_owned()),
    };
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("reading the pack archive failed: {error}"))?;
        let Some(enclosed_name) = entry.enclosed_name() else {
            continue;
        };
        let relative = enclosed_name.clone();
        let relative: Option<PathBuf> = match &strip_prefix {
            Some(prefix) => relative.strip_prefix(prefix).ok().map(PathBuf::from),
            None => Some(relative),
        };
        let Some(relative) = relative.filter(|path| !path.as_os_str().is_empty()) else {
            continue;
        };
        let target = destination.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
            entry
                .read_to_end(&mut bytes)
                .map_err(|error| format!("extracting the pack failed: {error}"))?;
            std::fs::write(&target, bytes).map_err(|error| error.to_string())?;
        }
    }
    Ok(destination.join("manifest.json"))
}

/// Verifies a downloaded archive against its pinned hash, extracts it, and
/// routes the manifest through the standard verified-install + activation
/// path. Returns the manifest path on success (caller activates).
pub fn stage_verified_pack_archive(
    archive: &Path,
    expected_sha256: &str,
    staging: &Path,
) -> Result<PathBuf, String> {
    let actual = sha256_file(archive)?;
    if !expected_sha256.is_empty() && !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(format!(
            "the downloaded pack hash does not match the pinned release hash (expected {expected_sha256}, got {actual})"
        ));
    }
    if expected_sha256.is_empty() {
        return Err("this pack has no pinned release hash; refusing to install".to_owned());
    }
    if staging.exists() {
        std::fs::remove_dir_all(staging).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(staging).map_err(|error| error.to_string())?;
    extract_pack_archive(archive, staging)
}

/// Streams the pinned archive to `destination`, reporting progress through
/// `on_progress(downloaded, total)` where `total` is known up front.
pub async fn download_pinned_archive(
    url: &str,
    expected_size: u64,
    destination: &Path,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), String> {
    let response = reqwest::get(url)
        .await
        .map_err(|error| format!("downloading the engine pack failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "the engine pack download returned HTTP {}",
            response.status()
        ));
    }
    let total = response.content_length().unwrap_or(expected_size);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(destination).map_err(|error| error.to_string())?,
    );
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream
        .next()
        .await
        .transpose()
        .map_err(|error| format!("downloading the engine pack failed: {error}"))?
    {
        file.write_all(&chunk)
            .map_err(|error| format!("writing the engine pack failed: {error}"))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    file.flush().map_err(|error| error.to_string())?;
    if downloaded == 0 {
        return Err("the engine pack download was empty".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_with_manifest(entries: &[(&str, &str)]) -> PathBuf {
        let directory = tempfile::tempdir().expect("tempdir");
        let archive = directory.path().join("pack.zip");
        let file = std::fs::File::create(&archive).expect("create zip");
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        for (name, contents) in entries {
            zip.start_file(*name, options).expect("start entry");
            zip.write_all(contents.as_bytes()).expect("write entry");
        }
        zip.finish().expect("finish zip");
        let _ = directory.keep();
        archive
    }

    #[test]
    fn hash_mismatch_and_unpinned_hashes_are_refused() {
        let archive = zip_with_manifest(&[("manifest.json", "{}")]);
        let staging = tempfile::tempdir().expect("staging");
        let error = stage_verified_pack_archive(
            &archive,
            "0000000000000000000000000000000000000000000000000000000000000000",
            staging.path(),
        )
        .expect_err("mismatch refused");
        assert!(error.contains("does not match"), "{error}");

        let error = stage_verified_pack_archive(&archive, "", staging.path())
            .expect_err("unpinned refused");
        assert!(error.contains("no pinned release hash"), "{error}");
    }

    #[test]
    fn archive_with_root_and_nested_manifest_extracts_to_staging() {
        let archive = zip_with_manifest(&[(
            "document-pack/manifest.json",
            r#"{"engine_id":"formatwright-document"}"#,
        )]);
        let staging = tempfile::tempdir().expect("staging");
        let sha = sha256_file(&archive).expect("hash");
        let manifest = stage_verified_pack_archive(&archive, &sha, staging.path()).expect("staged");
        assert!(manifest.ends_with("manifest.json"));
        let contents = std::fs::read_to_string(&manifest).expect("read manifest");
        assert!(contents.contains("formatwright-document"));
    }

    #[test]
    fn document_pack_announced_but_not_downloadable_until_pinned() {
        assert!(DOCUMENT_PACK.archive_sha256.is_empty());
        let views = optional_pack_views(Path::new("does-not-exist"));
        let document = views
            .iter()
            .find(|view| view.pack_id == "document")
            .expect("document pack listed");
        assert!(
            !document.downloadable,
            "unpinned pack must disable download"
        );
        assert!(!document.installed);
    }
}
