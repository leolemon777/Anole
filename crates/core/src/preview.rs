//! In-app output preview thumbnails (spec E-09).
//!
//! Renders one small image (max edge 256 px) for a finished output so the UI
//! can show the result without leaving the app. Preview generation follows the
//! runner-lane subprocess discipline: typed argv, bounded waits, process-tree
//! kill on timeout. A missing engine is not an error — previews degrade to
//! `None` and the UI hides the block instead of nagging the user.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

use crate::doctor::{EngineDiscoveryPolicy, inspect_engine_with_policy};
use crate::error::Result;

/// Longest edge of a generated preview, in pixels.
pub const PREVIEW_MAX_EDGE: u16 = 256;
/// Largest source image returned inline without re-encoding.
const MAX_INLINE_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
/// Wall-clock budget for one preview subprocess.
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(20);

/// A preview payload ready for inline display.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputPreview {
    pub mime_type: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
enum PreviewKind {
    /// Image formats `WebView2` renders natively; returned inline.
    InlineImage(&'static str),
    /// Rendered first page through `pdftoppm`.
    Pdf,
    /// Extracted first frame through `ffmpeg`.
    Video,
}

fn classify(path: &Path) -> Option<PreviewKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" => PreviewKind::InlineImage("image/png"),
        "jpg" | "jpeg" => PreviewKind::InlineImage("image/jpeg"),
        "webp" => PreviewKind::InlineImage("image/webp"),
        "gif" => PreviewKind::InlineImage("image/gif"),
        "bmp" => PreviewKind::InlineImage("image/bmp"),
        "avif" => PreviewKind::InlineImage("image/avif"),
        "pdf" => PreviewKind::Pdf,
        "mp4" | "mkv" | "mov" | "webm" | "avi" | "m4v" => PreviewKind::Video,
        _ => return None,
    })
}

/// Generates an output preview, or `Ok(None)` when the output has no preview
/// lane, the required engine is unavailable, or rendering fails. Every miss is
/// reported on stderr so degraded previews stay diagnosable without turning
/// into user-facing errors.
///
/// # Errors
///
/// Only an internal engine-inspection failure surfaces as an error; preview
/// misses are intentionally `None`.
pub async fn generate_output_preview(path: &Path) -> Result<Option<OutputPreview>> {
    let Some(kind) = classify(path) else {
        return Ok(None);
    };
    match kind {
        PreviewKind::InlineImage(mime_type) => inline_image(path, mime_type).await,
        PreviewKind::Pdf => render_with_engine(path, "pdftoppm", &pdftoppm_argv(path)).await,
        PreviewKind::Video => render_with_engine(path, "ffmpeg", &ffmpeg_argv(path)).await,
    }
}

async fn inline_image(path: &Path, mime_type: &'static str) -> Result<Option<OutputPreview>> {
    let bound = path.to_owned();
    let read = tokio::task::spawn_blocking(move || {
        let metadata = std::fs::metadata(&bound)?;
        if metadata.len() > MAX_INLINE_IMAGE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "preview inline image exceeds the size bound",
            ));
        }
        std::fs::read(&bound)
    })
    .await
    .map_err(|error| {
        crate::error::FormatWrightError::new(
            crate::error::ErrorCode::Internal,
            crate::error::Stage::Validate,
            "Output preview reader worker failed",
            "Retry opening the report.",
        )
        .with_diagnostic(error.to_string())
    })?;
    match read {
        Ok(bytes) => Ok(Some(OutputPreview { mime_type, bytes })),
        Err(error) => {
            eprintln!("output preview unavailable for {}: {error}", path.display());
            Ok(None)
        }
    }
}

fn pdftoppm_argv(pdf: &Path) -> Vec<String> {
    // Output prefix is replaced by the caller with a temp path; argv stays
    // typed and positional exactly like the validation lanes.
    vec![
        "-png".to_owned(),
        "-f".to_owned(),
        "1".to_owned(),
        "-l".to_owned(),
        "1".to_owned(),
        "-singlefile".to_owned(),
        "-scale-to".to_owned(),
        PREVIEW_MAX_EDGE.to_string(),
        pdf.to_string_lossy().into_owned(),
        "__PREVIEW_PREFIX__".to_owned(),
    ]
}

fn ffmpeg_argv(video: &Path) -> Vec<String> {
    vec![
        "-i".to_owned(),
        video.to_string_lossy().into_owned(),
        "-frames:v".to_owned(),
        "1".to_owned(),
        "-vf".to_owned(),
        format!("scale={PREVIEW_MAX_EDGE}:-2"),
        "__PREVIEW_OUTPUT__".to_owned(),
    ]
}

async fn render_with_engine(
    path: &Path,
    executable: &str,
    argv_template: &[String],
) -> Result<Option<OutputPreview>> {
    let policy = EngineDiscoveryPolicy::for_current_build();
    let engine = match inspect_engine_with_policy(executable, policy).await {
        Ok(engine) => engine,
        Err(error) => {
            // An absent engine degrades to a hidden preview by design (E-09);
            // the doctor page already explains how to import engine packs.
            eprintln!("output preview skipped ({executable} unavailable): {error}");
            return Ok(None);
        }
    };
    let staging = match tempfile::tempdir() {
        Ok(staging) => staging,
        Err(error) => {
            eprintln!("output preview staging failed: {error}");
            return Ok(None);
        }
    };
    let is_pdf = executable == "pdftoppm";
    let output_path: PathBuf = if is_pdf {
        staging.path().join("preview")
    } else {
        staging.path().join("preview.jpg")
    };
    let mut argv: Vec<String> = argv_template.to_vec();
    if is_pdf {
        *argv.last_mut().expect("pdftoppm argv ends with the prefix") =
            output_path.to_string_lossy().into_owned();
    } else {
        *argv.last_mut().expect("ffmpeg argv ends with the output") =
            output_path.to_string_lossy().into_owned();
    }

    let mut command = Command::new(&engine.binary_path);
    command
        .args(&argv)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let rendered = tokio::time::timeout(PREVIEW_TIMEOUT, async {
        match command.status().await {
            Ok(status) if status.success() => {
                let preview = if is_pdf {
                    output_path.with_extension("png")
                } else {
                    output_path
                };
                let read = tokio::task::spawn_blocking(move || std::fs::read(preview)).await;
                match read {
                    Ok(Ok(bytes)) => Some(bytes),
                    Ok(Err(error)) => {
                        eprintln!("output preview could not read the staged image: {error}");
                        None
                    }
                    Err(error) => {
                        eprintln!("output preview reader worker failed: {error}");
                        None
                    }
                }
            }
            outcome => {
                eprintln!(
                    "output preview engine {executable} did not succeed: {outcome:?} for {}",
                    path.display()
                );
                None
            }
        }
    })
    .await;
    let bytes = match rendered {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(_) => {
            eprintln!(
                "output preview timed out after {}s for {}",
                PREVIEW_TIMEOUT.as_secs(),
                path.display()
            );
            return Ok(None);
        }
    };
    let mime_type = if is_pdf { "image/png" } else { "image/jpeg" };
    Ok(Some(OutputPreview { mime_type, bytes }))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn classify_routes_supported_outputs_only() {
        assert!(matches!(
            classify(Path::new("out.png")),
            Some(PreviewKind::InlineImage("image/png"))
        ));
        assert!(matches!(
            classify(Path::new("out.PDF")),
            Some(PreviewKind::Pdf)
        ));
        assert!(matches!(
            classify(Path::new("clip.mkv")),
            Some(PreviewKind::Video)
        ));
        assert_eq!(classify(Path::new("notes.docx")), None);
        assert_eq!(classify(Path::new("noext")), None);
    }

    #[tokio::test]
    async fn inline_images_return_bytes_and_large_or_missing_files_degrade() {
        let suite = tempfile::tempdir().expect("suite");
        let small = suite.path().join("small.png");
        std::fs::write(&small, b"\x89PNG-not-really-but-bounded").expect("write png");
        let preview = generate_output_preview(&small)
            .await
            .expect("preview call")
            .expect("small image previews inline");
        assert_eq!(preview.mime_type, "image/png");

        let missing = suite.path().join("gone.png");
        assert!(
            generate_output_preview(&missing)
                .await
                .expect("preview call")
                .is_none(),
            "missing file degrades to no preview"
        );

        let big = suite.path().join("big.png");
        let mut file = std::fs::File::create(&big).expect("create big");
        #[allow(clippy::cast_possible_truncation)] // test-only byte count on a 64-bit host
        let oversized = vec![0_u8; (MAX_INLINE_IMAGE_BYTES + 1) as usize];
        file.write_all(&oversized).expect("write big");
        assert!(
            generate_output_preview(&big)
                .await
                .expect("preview call")
                .is_none(),
            "oversized inline image degrades to no preview"
        );
    }

    #[tokio::test]
    async fn unknown_output_extensions_never_spawn_an_engine() {
        assert!(
            generate_output_preview(Path::new("archive.zip"))
                .await
                .expect("preview call")
                .is_none()
        );
    }
}
