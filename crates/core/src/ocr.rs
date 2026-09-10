use std::collections::BTreeMap;
use std::path::PathBuf;

use anole_engine_sdk::{EngineIdentity, LossClass, Operation};
use serde_json::json;
use uuid::Uuid;

use crate::domain::{
    ArtifactSummary, ChangeSet, NetworkPolicy, Plan, PlanStep, Probe, ReportRedaction,
    SCHEMA_VERSION, ValidationCheck, ValidationReport, ValidationStatus,
};
use crate::error::{AnoleError, ErrorCode, Result, Stage};
use crate::planner::deterministic_plan_hash;

/// Tesseract page segmentation mode for full-page automatic layout.
pub(crate) const OCR_PSM: u8 = 3;
/// Rasterization resolution used when OCR-ing PDF pages with pdftoppm.
pub(crate) const OCR_PDF_DPI: u16 = 150;

fn ensure_tesseract(tesseract: &EngineIdentity) -> Result<()> {
    if tesseract.engine_id != "tesseract" {
        return Err(AnoleError::new(
            ErrorCode::EngineIncompatible,
            Stage::Plan,
            "The OCR Plan was given the wrong engine",
            "Run doctor and use tesseract.",
        ));
    }
    Ok(())
}

fn normalize_ocr_language(raw: Option<&str>) -> Result<String> {
    let language = raw.unwrap_or("eng").trim().to_ascii_lowercase();
    if language.is_empty()
        || language.len() > 16
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    {
        return Err(AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Plan,
            format!("Unsupported OCR language code: {language:?}"),
            "Use a Tesseract language code such as eng or chi_sim.",
        ));
    }
    Ok(language)
}

fn ensure_image_probe(probe: &Probe) -> Result<()> {
    if !matches!(
        probe.format.id.as_str(),
        "png" | "jpg" | "jpeg" | "tiff" | "bmp"
    ) {
        return Err(AnoleError::new(
            ErrorCode::Unsupported,
            Stage::Plan,
            "Image OCR needs a PNG, JPEG, TIFF, or BMP input",
            "Retry with a raster image file.",
        ));
    }
    Ok(())
}

/// Plans a single-image OCR pass (operation-free route png/jpg -> txt/md): the
/// recognized text is a new, explicitly lossy artifact; md wraps the same
/// text in a Markdown file.
///
/// # Errors
///
/// Returns `Unsupported`/`EngineIncompatible` for non-raster inputs or a
/// non-tesseract engine identity.
pub fn plan_image_ocr(
    probe: &Probe,
    output_path: PathBuf,
    tesseract: &EngineIdentity,
    language: Option<&str>,
    target: &str,
) -> Result<Plan> {
    ensure_image_probe(probe)?;
    ensure_tesseract(tesseract)?;
    let target = target.trim().trim_start_matches('.').to_ascii_lowercase();
    if !matches!(target.as_str(), "txt" | "md") {
        return Err(AnoleError::new(
            ErrorCode::Unsupported,
            Stage::Plan,
            "Image OCR target must be txt or md",
            "Choose txt or md for OCR output.",
        ));
    }
    let language = normalize_ocr_language(language)?;
    let arguments = BTreeMap::from([
        // `ocr_mode` (not `operation`) keeps this plan off the ADR-0013
        // qpdf dispatch, which routes on the `operation` argument.
        ("ocr_mode".to_owned(), "image".to_owned()),
        ("source_format".to_owned(), probe.format.id.clone()),
        ("target_format".to_owned(), target.clone()),
        ("language".to_owned(), language),
        ("psm".to_owned(), OCR_PSM.to_string()),
    ]);
    ocr_plan(
        "tesseract.image-ocr",
        arguments,
        probe,
        output_path,
        tesseract,
        target.as_str(),
        vec!["ocr.text-nonempty".to_owned()],
    )
}

/// Plans a scanned-PDF OCR pass (ADR-0013 operation `pdf-ocr`): every page is
/// rasterized with pdftoppm at `OCR_PDF_DPI`, recognized by tesseract, and the
/// per-page text is concatenated into one txt output.
///
/// # Errors
///
/// Returns `Unsupported`/`EngineIncompatible` for non-PDF inputs or engines.
pub fn plan_pdf_ocr(
    probe: &Probe,
    output_path: PathBuf,
    tesseract: &EngineIdentity,
    language: Option<&str>,
) -> Result<Plan> {
    if probe.format.id != "pdf" {
        return Err(AnoleError::new(
            ErrorCode::Unsupported,
            Stage::Plan,
            "PDF OCR needs a PDF input",
            "Retry with a PDF file.",
        ));
    }
    ensure_tesseract(tesseract)?;
    let language = normalize_ocr_language(language)?;
    let page_count = u32::try_from(probe.streams.len()).unwrap_or(u32::MAX);
    let arguments = BTreeMap::from([
        ("operation".to_owned(), "pdf-ocr".to_owned()),
        ("ocr_mode".to_owned(), "pdf".to_owned()),
        ("dpi".to_owned(), OCR_PDF_DPI.to_string()),
        ("language".to_owned(), language),
        ("psm".to_owned(), OCR_PSM.to_string()),
        ("expected_pages".to_owned(), page_count.to_string()),
    ]);
    ocr_plan(
        "tesseract.pdf-ocr",
        arguments,
        probe,
        output_path,
        tesseract,
        "txt",
        vec![
            "ocr.text-nonempty".to_owned(),
            "ocr.page-coverage".to_owned(),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn ocr_plan(
    capability_id: &str,
    arguments: BTreeMap<String, String>,
    probe: &Probe,
    output_path: PathBuf,
    tesseract: &EngineIdentity,
    target: &str,
    validators: Vec<String>,
) -> Result<Plan> {
    let step = PlanStep {
        step_id: "step-1".to_owned(),
        capability_id: capability_id.to_owned(),
        engine: tesseract.clone(),
        operation: Operation::Transform,
        // Recognition is inherently lossy: the output certifies text, not the
        // original pixels.
        loss_class: LossClass::Lossy,
        arguments,
        estimated_temporary_bytes: Some(probe.artifact.size_bytes.saturating_mul(3)),
    };
    let mut plan = Plan {
        schema_version: SCHEMA_VERSION,
        plan_id: Uuid::new_v4(),
        plan_hash: String::new(),
        input_fingerprint: probe.artifact.fast_fingerprint.clone(),
        target_format: target.to_owned(),
        constraints: BTreeMap::from([
            ("network".to_owned(), json!("deny")),
            ("external_resources".to_owned(), json!("deny")),
        ]),
        steps: vec![step],
        changes: ChangeSet {
            preserved: vec![],
            changed: vec!["recognized text becomes the output artifact".to_owned()],
            dropped: vec!["all visual and layout information".to_owned()],
            unknown: vec!["OCR recognition accuracy is not certified".to_owned()],
        },
        validators,
        network_policy: NetworkPolicy::Deny,
        output_path: Some(output_path),
        estimated_output_bytes: None,
    };
    plan.plan_hash = deterministic_plan_hash(&plan)?;
    Ok(plan)
}

/// Returns whether an OCR text carries at least one alphanumeric token — the
/// required `OCR_TEXT_NONEMPTY` acceptance predicate.
pub(crate) fn ocr_text_nonempty(text: &str) -> bool {
    text.split_whitespace()
        .any(|token| token.chars().any(char::is_alphanumeric))
}

/// Builds the OCR acceptance report: `OCR_TEXT_NONEMPTY` (required) and, for
/// the pdf-ocr operation, `OCR_PAGE_COVERAGE` (required) — the number of pages
/// that completed recognition must equal the pdfinfo page count.
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_ocr_output(
    input: &Probe,
    output_identity: &crate::domain::ArtifactIdentity,
    plan: &Plan,
    job_id: Uuid,
    text: &str,
    page_coverage: Option<(u32, u32)>,
) -> ValidationReport {
    let mut checks = vec![ValidationCheck {
        code: "OCR_TEXT_NONEMPTY".to_owned(),
        status: if ocr_text_nonempty(text) {
            ValidationStatus::Pass
        } else {
            ValidationStatus::Fail
        },
        required: true,
        expected: json!("at least one alphanumeric token"),
        observed: json!(text.len()),
        evidence: "Tesseract 5 stdout".to_owned(),
        message: "The recognized text output must not be empty.".to_owned(),
    }];
    if let Some((processed, expected)) = page_coverage {
        checks.push(ValidationCheck {
            code: "OCR_PAGE_COVERAGE".to_owned(),
            status: if processed == expected && expected > 0 {
                ValidationStatus::Pass
            } else {
                ValidationStatus::Fail
            },
            required: true,
            expected: json!(expected),
            observed: json!(processed),
            evidence: "pdftoppm page rasterization".to_owned(),
            message: "Every input page must be rasterized and recognized.".to_owned(),
        });
    }
    let report_status = checks.iter().fold(ValidationStatus::Pass, |state, check| {
        state.worst(check.status)
    });
    ValidationReport {
        schema_version: SCHEMA_VERSION,
        report_id: Uuid::new_v4(),
        job_id,
        plan_hash: plan.plan_hash.clone(),
        status: report_status,
        input: ArtifactSummary {
            display_path: Some(input.artifact.display_path.clone()),
            format_id: input.format.id.clone(),
            size_bytes: input.artifact.size_bytes,
            fast_fingerprint: input.artifact.fast_fingerprint.clone(),
            full_blake3: input.artifact.full_blake3.clone(),
        },
        output: ArtifactSummary {
            display_path: Some(output_identity.display_path.clone()),
            format_id: "txt".to_owned(),
            size_bytes: output_identity.size_bytes,
            fast_fingerprint: output_identity.fast_fingerprint.clone(),
            full_blake3: output_identity.full_blake3.clone(),
        },
        engines: plan.steps.iter().map(|step| step.engine.clone()).collect(),
        checks,
        intentional_changes: plan.changes.changed.clone(),
        redaction: ReportRedaction {
            paths_redacted: false,
            metadata_values_redacted: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_ocr_language, ocr_text_nonempty, plan_image_ocr};

    fn tesseract_engine() -> anole_engine_sdk::EngineIdentity {
        use anole_engine_sdk::{Certification, EngineIdentity};
        EngineIdentity {
            engine_id: "tesseract".to_owned(),
            version: "5.4".to_owned(),
            binary_path: std::path::PathBuf::from("tesseract"),
            binary_sha256: "0".repeat(64),
            manifest_sha256: None,
            build_configuration: None,
            certification: Certification::Experimental,
        }
    }

    #[test]
    fn ocr_language_codes_validate_and_default_to_eng() {
        assert_eq!(normalize_ocr_language(None).expect("default"), "eng");
        assert_eq!(
            normalize_ocr_language(Some(" CHI_sim ")).expect("normalized"),
            "chi_sim"
        );
        assert!(normalize_ocr_language(Some("")).is_err());
        assert!(normalize_ocr_language(Some("eng;rm")).is_err());
        assert!(normalize_ocr_language(Some("too-long-code")).is_err());
    }

    #[test]
    fn nonempty_requires_an_alphanumeric_token() {
        assert!(ocr_text_nonempty("OCR TEST ELECTRIC 440010147700"));
        assert!(ocr_text_nonempty("\n  123 \n"));
        assert!(!ocr_text_nonempty(""));
        assert!(!ocr_text_nonempty("   \n\t --- \r\n"));
    }

    fn image_probe(name: &str) -> crate::domain::Probe {
        use crate::domain::{ArtifactIdentity, FormatDescriptor, FormatKind, Probe, ProbeEvidence};
        use std::collections::BTreeMap;
        Probe {
            schema_version: crate::domain::SCHEMA_VERSION,
            artifact: ArtifactIdentity {
                display_path: name.to_owned(),
                canonical_path: std::path::PathBuf::from(name),
                size_bytes: 1024,
                modified_unix_ms: 0,
                fast_fingerprint: "blake3:probe".to_owned(),
                full_blake3: None,
            },
            format: FormatDescriptor {
                id: "png".to_owned(),
                kind: FormatKind::Image,
                mime_type: Some("image/png".to_owned()),
                container: None,
                extension_matches: Some(true),
                confidence: 1.0,
            },
            streams: Vec::new(),
            metadata: BTreeMap::new(),
            warnings: Vec::new(),
            evidence: ProbeEvidence {
                engine_id: "ffprobe".to_owned(),
                engine_version: "test".to_owned(),
                engine_binary_sha256: None,
            },
            duration_seconds: None,
            bit_rate: None,
        }
    }

    #[test]
    fn image_ocr_plan_requires_a_raster_input() {
        let engine = tesseract_engine();
        let probe = image_probe("input.png");
        let plan = plan_image_ocr(
            &probe,
            std::path::PathBuf::from("out.txt"),
            &engine,
            None,
            "txt",
        )
        .expect("plan");
        assert_eq!(plan.target_format, "txt");
        assert_eq!(plan.steps[0].engine.engine_id, "tesseract");
        assert_eq!(plan.steps[0].arguments["ocr_mode"], "image");
        assert!(
            !plan.steps[0].arguments.contains_key("operation"),
            "image OCR must not ride the qpdf operation dispatch"
        );
        assert_eq!(plan.steps[0].loss_class, anole_engine_sdk::LossClass::Lossy);
    }

    #[test]
    fn image_ocr_plan_accepts_md_target() {
        let engine = tesseract_engine();
        let probe = image_probe("scan.png");
        let plan = plan_image_ocr(
            &probe,
            std::path::PathBuf::from("out.md"),
            &engine,
            None,
            "md",
        )
        .expect("plan");
        assert_eq!(plan.target_format, "md");
        assert_eq!(plan.steps[0].arguments["target_format"], "md");
        assert!(
            plan_image_ocr(
                &probe,
                std::path::PathBuf::from("out.pdf"),
                &engine,
                None,
                "pdf"
            )
            .is_err(),
            "OCR output beyond txt/md must be rejected"
        );
    }
}
