use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

use anole_engine_sdk::{EngineIdentity, LossClass, Operation};
use calamine::Reader as CalamineReader;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Value, json};
use uuid::Uuid;
use zip::ZipArchive;

use crate::domain::{
    ArtifactSummary, ChangeSet, DiagnosticMessage, FormatDescriptor, FormatKind, NetworkPolicy,
    Plan, PlanStep, Probe, ProbeEvidence, ReportRedaction, SCHEMA_VERSION, StreamKind, StreamProbe,
    ValidationCheck, ValidationReport, ValidationStatus,
};
use crate::error::{AnoleError, ErrorCode, Result, Stage};
use crate::fingerprint::identify_artifact;
use crate::planner::deterministic_plan_hash;

const MAX_OFFICE_ENTRIES: usize = 10_000;
const MAX_OFFICE_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_RELATIONSHIP_BYTES: u64 = 8 * 1024 * 1024;

/// Detects an OOXML family from ZIP package parts instead of its extension.
///
/// # Errors
///
/// Returns a typed input/resource error when an office-looking package is
/// malformed or exceeds bounded package limits.
pub fn office_format_hint(path: impl AsRef<Path>) -> Result<Option<&'static str>> {
    let path = path.as_ref();
    let mut prefix = [0_u8; 6];
    let read = File::open(path)
        .and_then(|mut file| file.read(&mut prefix))
        .map_err(|error| input_error(path, &error))?;
    if read >= 5 && &prefix[..5] == b"{\\rtf" {
        return Ok(Some("rtf"));
    }
    if read < 4 || prefix[..4] != *b"PK\x03\x04" {
        return Ok(None);
    }
    let extension_is_office = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "docx" | "pptx" | "xlsx" | "odt" | "ods" | "odp"
            )
        });
    let file = File::open(path).map_err(|error| input_error(path, &error))?;
    let mut archive = match ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(_) if !extension_is_office => return Ok(None),
        Err(error) => return Err(invalid_zip(&error)),
    };
    check_package_limits(&mut archive)?;
    Ok(detect_package_family(&mut archive))
}

/// Inspects a `DOCX`/`PPTX`/`XLSX`/ODF package or RTF envelope without
/// executing macros or external links.
///
/// # Errors
///
/// Returns a typed error for unknown, malformed, macro-bearing, externally
/// linked, or resource-exhausting packages.
#[allow(clippy::too_many_lines)]
pub async fn inspect_office(path: impl AsRef<Path>) -> Result<Probe> {
    let path = path.as_ref();
    let artifact = identify_artifact(path).await?;
    let owned = artifact.canonical_path.clone();
    let is_rtf = office_format_hint(path).ok().flatten() == Some("rtf");
    let inspection = tokio::task::spawn_blocking(move || {
        if is_rtf {
            Ok(inspect_rtf_envelope(&owned))
        } else {
            inspect_package(&owned)
        }
    })
    .await
    .map_err(|error| {
        AnoleError::new(
            ErrorCode::Internal,
            Stage::Inspect,
            "Office inspection worker failed",
            "Retry or report the input.",
        )
        .with_diagnostic(error.to_string())
    })??;
    if inspection.has_macros {
        return Err(AnoleError::new(
            ErrorCode::PolicyBlocked,
            Stage::Inspect,
            "Office package contains a VBA project",
            "Remove macros in an isolated trusted editor, then retry with macro-free OOXML.",
        ));
    }
    if inspection.has_external_relationships {
        return Err(AnoleError::new(
            ErrorCode::PolicyBlocked,
            Stage::Inspect,
            "Office package contains external relationships under deny-all policy",
            "Embed or remove external resources, then retry.",
        ));
    }
    let extension = artifact
        .canonical_path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    let extension_matches = extension.as_deref() == Some(inspection.format);
    let properties = BTreeMap::from([
        ("package_entries".to_owned(), json!(inspection.entry_count)),
        (
            "expanded_bytes".to_owned(),
            json!(inspection.expanded_bytes),
        ),
        ("required_part_present".to_owned(), json!(true)),
        ("has_macros".to_owned(), json!(false)),
        ("has_external_relationships".to_owned(), json!(false)),
    ]);
    Ok(Probe {
        schema_version: SCHEMA_VERSION,
        artifact,
        format: FormatDescriptor {
            id: inspection.format.to_owned(),
            kind: FormatKind::Document,
            mime_type: Some(mime_type(inspection.format).to_owned()),
            container: Some(if inspection.format == "rtf" {
                "text/rtf".to_owned()
            } else if inspection.format.starts_with("od") {
                "zip/odf".to_owned()
            } else {
                "zip/opc".to_owned()
            }),
            extension_matches: Some(extension_matches),
            confidence: 1.0,
        },
        streams: vec![StreamProbe {
            index: 0,
            kind: if inspection.format == "xlsx" {
                StreamKind::RecordSet
            } else {
                StreamKind::Page
            },
            codec: Some(if inspection.format == "rtf" {
                "rtf".to_owned()
            } else if inspection.format.starts_with("od") {
                "odf".to_owned()
            } else {
                "ooxml".to_owned()
            }),
            language: None,
            duration_seconds: None,
            width: None,
            height: None,
            frame_rate: None,
            sample_rate: None,
            channels: None,
            properties,
        }],
        metadata: BTreeMap::new(),
        warnings: if extension_matches {
            Vec::new()
        } else {
            vec![DiagnosticMessage {
                code: "EXTENSION_MISMATCH".to_owned(),
                severity: "warning".to_owned(),
                message: format!(
                    "File content is {} but its extension does not match",
                    inspection.format
                ),
            }]
        },
        evidence: ProbeEvidence {
            engine_id: "anole.office-inspector".to_owned(),
            engine_version: env!("CARGO_PKG_VERSION").to_owned(),
            engine_binary_sha256: None,
        },
        duration_seconds: None,
        bit_rate: None,
    })
}

/// Plans macro-disabled `LibreOffice` conversion and all-page PDF render validation.
///
/// # Errors
///
/// Returns a planning error for unsupported inputs or incorrect engines.
#[allow(clippy::too_many_lines)]
pub fn plan_office_to_pdf(
    probe: &Probe,
    output_path: std::path::PathBuf,
    soffice: &EngineIdentity,
    pdfinfo: &EngineIdentity,
    pdftoppm: &EngineIdentity,
) -> Result<Plan> {
    if !matches!(
        probe.format.id.as_str(),
        "docx" | "pptx" | "xlsx" | "odt" | "ods" | "odp" | "rtf"
    ) {
        return Err(unsupported(
            "Office-to-PDF requires OOXML, ODF, or RTF input",
        ));
    }
    if soffice.engine_id != "soffice"
        || pdfinfo.engine_id != "pdfinfo"
        || pdftoppm.engine_id != "pdftoppm"
    {
        return Err(AnoleError::new(
            ErrorCode::EngineIncompatible,
            Stage::Plan,
            "Office-to-PDF Plan was given an incorrect engine",
            "Run doctor and use soffice, pdfinfo, and pdftoppm.",
        ));
    }
    let conversion = PlanStep {
        step_id: "step-1".to_owned(),
        capability_id: format!("libreoffice.{}-to-pdf.headless", probe.format.id),
        engine: soffice.clone(),
        operation: Operation::Render,
        loss_class: LossClass::Unknown,
        arguments: BTreeMap::from([
            ("source_format".to_owned(), probe.format.id.clone()),
            ("target_format".to_owned(), "pdf".to_owned()),
            ("headless".to_owned(), "true".to_owned()),
            ("isolated_profile".to_owned(), "true".to_owned()),
            ("macros".to_owned(), "disabled".to_owned()),
            ("external_resources".to_owned(), "deny".to_owned()),
        ]),
        estimated_temporary_bytes: Some(probe.artifact.size_bytes.saturating_mul(8)),
    };
    let structural_validation = PlanStep {
        step_id: "step-2".to_owned(),
        capability_id: "poppler.pdf-structural-validation.all-pages".to_owned(),
        engine: pdfinfo.clone(),
        operation: Operation::Inspect,
        loss_class: LossClass::None,
        arguments: BTreeMap::from([
            ("page_sizes".to_owned(), "required".to_owned()),
            ("target_format".to_owned(), "pdf".to_owned()),
            ("purpose".to_owned(), "validation-only".to_owned()),
        ]),
        estimated_temporary_bytes: None,
    };
    let render_validation = PlanStep {
        step_id: "step-3".to_owned(),
        capability_id: "poppler.pdf-render-validation.all-pages".to_owned(),
        engine: pdftoppm.clone(),
        operation: Operation::Inspect,
        loss_class: LossClass::None,
        arguments: BTreeMap::from([
            ("dpi".to_owned(), "72".to_owned()),
            ("target_format".to_owned(), "png".to_owned()),
            ("purpose".to_owned(), "validation-only".to_owned()),
        ]),
        estimated_temporary_bytes: None,
    };
    let mut plan = Plan {
        schema_version: SCHEMA_VERSION,
        plan_id: Uuid::new_v4(),
        plan_hash: String::new(),
        input_fingerprint: probe.artifact.fast_fingerprint.clone(),
        target_format: "pdf".to_owned(),
        constraints: BTreeMap::from([
            ("network".to_owned(), json!("deny")),
            ("macros".to_owned(), json!("disabled")),
            ("external_resources".to_owned(), json!("deny")),
            ("isolated_user_profile".to_owned(), json!(true)),
            ("all_pdf_pages_must_render".to_owned(), json!(true)),
        ]),
        steps: vec![conversion, structural_validation, render_validation],
        changes: ChangeSet {
            preserved: vec![
                "visible document content supported by LibreOffice".to_owned(),
                "page order produced by the isolated office renderer".to_owned(),
            ],
            changed: vec![
                "editable Office structure rendered into fixed PDF pages".to_owned(),
                "active content disabled".to_owned(),
            ],
            dropped: vec![
                "macros, editable formulas/objects, transitions, and interactive behavior"
                    .to_owned(),
            ],
            unknown: vec![
                "font substitution and pixel-level layout fidelity require fixture comparison"
                    .to_owned(),
            ],
        },
        validators: vec![
            "office.pdf-opens".to_owned(),
            "office.pdf-page-count".to_owned(),
            "office.pdf-page-sizes".to_owned(),
            "office.pdf-all-pages-render".to_owned(),
            "office.font-diagnostics".to_owned(),
            "office.visual-drift".to_owned(),
        ],
        network_policy: NetworkPolicy::Deny,
        output_path: Some(output_path),
        estimated_output_bytes: None,
    };
    plan.plan_hash = deterministic_plan_hash(&plan)?;
    Ok(plan)
}

/// Plans a macro-disabled `LibreOffice` DOCX <-> ODT document exchange.
///
/// # Errors
///
/// Returns a planning error for unsupported inputs/targets or a wrong engine.
pub fn plan_office_document_exchange(
    probe: &Probe,
    output_path: std::path::PathBuf,
    soffice: &EngineIdentity,
    target: &str,
) -> Result<Plan> {
    let target = target.trim().trim_start_matches('.').to_ascii_lowercase();
    if !matches!(probe.format.id.as_str(), "docx" | "odt") {
        return Err(unsupported(
            "Document exchange requires DOCX or ODF text input",
        ));
    }
    if probe.format.id == target {
        return Err(AnoleError::new(
            ErrorCode::Unsupported,
            Stage::Plan,
            format!("Document exchange cannot convert {target} to itself"),
            "Choose the opposite document format.",
        ));
    }
    if !matches!(target.as_str(), "docx" | "odt") {
        return Err(unsupported("Document exchange target must be DOCX or ODT"));
    }
    if soffice.engine_id != "soffice" {
        return Err(AnoleError::new(
            ErrorCode::EngineIncompatible,
            Stage::Plan,
            "Document exchange Plan was given an incorrect engine",
            "Run doctor and use soffice.",
        ));
    }
    let conversion = PlanStep {
        step_id: "step-1".to_owned(),
        capability_id: format!("libreoffice.{}-to-{}.headless", probe.format.id, target),
        engine: soffice.clone(),
        operation: Operation::Transform,
        loss_class: LossClass::Unknown,
        arguments: BTreeMap::from([
            ("source_format".to_owned(), probe.format.id.clone()),
            ("target_format".to_owned(), target.clone()),
            ("headless".to_owned(), "true".to_owned()),
            ("isolated_profile".to_owned(), "true".to_owned()),
            ("macros".to_owned(), "disabled".to_owned()),
            ("external_resources".to_owned(), "deny".to_owned()),
        ]),
        estimated_temporary_bytes: Some(probe.artifact.size_bytes.saturating_mul(8)),
    };
    let mut plan = Plan {
        schema_version: SCHEMA_VERSION,
        plan_id: Uuid::new_v4(),
        plan_hash: String::new(),
        input_fingerprint: probe.artifact.fast_fingerprint.clone(),
        target_format: target,
        constraints: BTreeMap::from([
            ("network".to_owned(), json!("deny")),
            ("macros".to_owned(), json!("disabled")),
            ("external_resources".to_owned(), json!("deny")),
            ("isolated_user_profile".to_owned(), json!(true)),
        ]),
        steps: vec![conversion],
        changes: ChangeSet {
            preserved: vec![
                "visible document content supported by LibreOffice".to_owned(),
                "body text order produced by the isolated office converter".to_owned(),
            ],
            changed: vec![
                "package structure is rewritten between OOXML and ODF containers".to_owned(),
            ],
            dropped: vec!["macros and interactive behavior".to_owned()],
            unknown: vec![
                "application-specific styling fidelity requires fixture comparison".to_owned(),
            ],
        },
        validators: vec![
            "office.package-opens".to_owned(),
            "office.target-structure".to_owned(),
        ],
        network_policy: NetworkPolicy::Deny,
        output_path: Some(output_path),
        estimated_output_bytes: None,
    };
    plan.plan_hash = deterministic_plan_hash(&plan)?;
    Ok(plan)
}

/// Plans the built-in XLSX data export: every worksheet becomes one CSV
/// file under a paged output directory (the pdf→png precedent).
///
/// 引擎是内置 `anole.office-csv`（calamine 纯 Rust 解析）——不依赖
/// LibreOffice，公式读出的是工作簿缓存的计算值；多 sheet 在
/// `constraints` 里声明为 all worksheets。
///
/// # Errors
///
/// Returns a planning error for non-XLSX input or a wrong engine.
pub fn plan_office_csv_export(
    probe: &Probe,
    output_path: std::path::PathBuf,
    engine: &EngineIdentity,
) -> Result<Plan> {
    if probe.format.id != "xlsx" {
        return Err(unsupported(
            "Office CSV export requires XLSX spreadsheet input",
        ));
    }
    if engine.engine_id != "anole.office-csv" {
        return Err(AnoleError::new(
            ErrorCode::EngineIncompatible,
            Stage::Plan,
            "Office CSV export Plan was given an incorrect engine",
            "Run doctor and use the built-in anole.office-csv engine.",
        ));
    }
    let conversion = PlanStep {
        step_id: "step-1".to_owned(),
        capability_id: "anole.office-csv.xlsx-all-sheets".to_owned(),
        engine: engine.clone(),
        operation: Operation::Transform,
        loss_class: LossClass::Lossy,
        arguments: BTreeMap::from([
            ("source_format".to_owned(), "xlsx".to_owned()),
            ("target_format".to_owned(), "csv".to_owned()),
            ("worksheets".to_owned(), "all".to_owned()),
            ("formulas".to_owned(), "cached-values".to_owned()),
        ]),
        estimated_temporary_bytes: Some(probe.artifact.size_bytes.saturating_mul(4)),
    };
    let mut plan = Plan {
        schema_version: SCHEMA_VERSION,
        plan_id: Uuid::new_v4(),
        plan_hash: String::new(),
        input_fingerprint: probe.artifact.fast_fingerprint.clone(),
        target_format: "csv".to_owned(),
        constraints: BTreeMap::from([
            ("network".to_owned(), json!("deny")),
            ("external_resources".to_owned(), json!("deny")),
            (
                "worksheets".to_owned(),
                json!("all worksheets, one CSV per sheet"),
            ),
            (
                "output_shape".to_owned(),
                json!("paged directory of CSV files"),
            ),
        ]),
        steps: vec![conversion],
        changes: ChangeSet {
            preserved: vec![
                "cell values of every worksheet as cached by the workbook".to_owned(),
                "row order and sheet order".to_owned(),
            ],
            changed: vec![
                "formulas are exported as their cached computed values".to_owned(),
                "each worksheet is flattened to its own text grid".to_owned(),
            ],
            dropped: vec!["cell styling, number formats, charts, and macros".to_owned()],
            unknown: vec![
                "cells whose cached value predates the last edit show the cache".to_owned(),
            ],
        },
        validators: vec![
            "office.csv-opens".to_owned(),
            "office.csv-sheet-count".to_owned(),
            "office.csv-rows-present".to_owned(),
        ],
        network_policy: NetworkPolicy::Deny,
        output_path: Some(output_path),
        estimated_output_bytes: None,
    };
    plan.plan_hash = deterministic_plan_hash(&plan)?;
    Ok(plan)
}

/// 内置 calamine 执行：每张工作表写一个 `sheet-NN[-name].csv` 到
/// `output_dir`。返回写出的 sheet 数（验收用）。公式单元格读缓存值。
pub(crate) fn convert_xlsx_to_csv_sheets(
    input: &Path,
    output_dir: &Path,
    plan: &Plan,
) -> Result<usize> {
    let step = plan.steps.first().ok_or_else(|| {
        AnoleError::new(
            ErrorCode::Internal,
            Stage::Execute,
            "Office CSV export Plan has no conversion step",
            "Preview the plan again.",
        )
    })?;
    if !matches!(
        step.arguments.get("worksheets").map(String::as_str),
        Some("all")
    ) || !matches!(
        step.arguments.get("formulas").map(String::as_str),
        Some("cached-values")
    ) {
        return Err(AnoleError::new(
            ErrorCode::Internal,
            Stage::Execute,
            "Office CSV export Plan carries unexpected arguments",
            "Preview the plan again.",
        ));
    }
    let mut workbook = calamine::open_workbook_auto(input).map_err(|error| {
        AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Execute,
            "The XLSX workbook cannot be opened for CSV export",
            "Re-save the workbook as .xlsx and retry.",
        )
        .with_diagnostic(error.to_string())
    })?;
    let sheet_names = workbook.sheet_names().clone();
    if sheet_names.is_empty() {
        return Err(AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Execute,
            "The XLSX workbook has no worksheets",
            "Add a worksheet and retry.",
        ));
    }
    std::fs::create_dir_all(output_dir).map_err(|error| {
        AnoleError::new(
            ErrorCode::StorageFailed,
            Stage::Execute,
            "Unable to create the CSV export directory",
            "Check destination permissions and storage health.",
        )
        .with_diagnostic(error.to_string())
    })?;
    for (index, sheet_name) in sheet_names.iter().enumerate() {
        let range = workbook.worksheet_range(sheet_name).map_err(|error| {
            AnoleError::new(
                ErrorCode::InputInvalid,
                Stage::Execute,
                format!("Worksheet {sheet_name} cannot be read"),
                "Inspect the workbook and retry.",
            )
            .with_diagnostic(error.to_string())
        })?;
        let file_name = format!(
            "sheet-{:02}{}.csv",
            index + 1,
            sanitize_sheet_suffix(sheet_name)
        );
        let destination = output_dir.join(file_name);
        let file = std::fs::File::create(&destination).map_err(|error| {
            AnoleError::new(
                ErrorCode::StorageFailed,
                Stage::Execute,
                "Unable to create a CSV sheet export",
                "Check destination permissions and storage health.",
            )
            .with_diagnostic(error.to_string())
        })?;
        let mut writer = csv::WriterBuilder::new().from_writer(file);
        for row in range.rows() {
            let record = row.iter().map(calamine_cell_text);
            writer
                .write_record(record)
                .map_err(|error| csv_write_error(&error))?;
        }
        writer.flush().map_err(|error| {
            AnoleError::new(
                ErrorCode::StorageFailed,
                Stage::Execute,
                "Unable to finalize a CSV sheet export",
                "Check destination permissions and storage health.",
            )
            .with_diagnostic(error.to_string())
        })?;
    }
    Ok(sheet_names.len())
}

fn csv_write_error(error: &csv::Error) -> AnoleError {
    AnoleError::new(
        ErrorCode::StorageFailed,
        Stage::Execute,
        "Unable to write a CSV sheet export",
        "Check destination permissions and storage health.",
    )
    .with_diagnostic(error.to_string())
}

/// sheet 文件名后缀：保留 Unicode 字母数字与连字符/下划线，其余折叠
/// 为 `_`，限长 32（按字符计），避免跨文件系统字符问题。
fn sanitize_sheet_suffix(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .take(32)
        .map(|character| {
            if character.is_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        String::new()
    } else {
        format!("-{cleaned}")
    }
}

fn calamine_cell_text(cell: &calamine::Data) -> String {
    use calamine::Data;
    match cell {
        Data::Empty => String::new(),
        Data::String(value) => value.clone(),
        Data::Float(value) => {
            // 整数值浮点（Excel 数字常态）格式化为无小数点文本；
            // 非整数走 Debug 风格的 to_string，不经数值截断。
            if value.fract() == 0.0 && value.abs() < 1e15 {
                format!("{value}")
            } else {
                value.to_string()
            }
        }
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        other => other.to_string(),
    }
}

/// Validation for the built-in XLSX → CSV export: every produced sheet CSV
/// must re-open as UTF-8 CSV, the sheet-file count must equal the workbook's
/// sheet count, and at least one row must exist across all sheets.
pub(crate) fn validate_office_csv_output(
    input: &Probe,
    output_dir: &Path,
    plan: &Plan,
    job_id: Uuid,
) -> ValidationReport {
    let mut workbook = calamine::open_workbook_auto(&input.artifact.canonical_path).ok();
    let expected_sheets = workbook.as_mut().map_or(0, |book| book.sheet_names().len());
    let mut entries = std::fs::read_dir(output_dir)
        .map(|directory| {
            directory
                .flatten()
                .filter_map(|entry| {
                    entry
                        .path()
                        .extension()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| value.eq_ignore_ascii_case("csv"))
                        .then(|| entry.path())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    let observed_sheets = entries.len();
    let mut parse_failures = Vec::new();
    let mut total_rows = 0_usize;
    for entry in &entries {
        match count_csv_rows(entry) {
            Ok((rows, _)) => total_rows += rows,
            Err(error) => parse_failures.push(format!("{}: {error}", entry.display())),
        }
    }
    let parses = parse_failures.is_empty();
    let sheet_count_ok = observed_sheets == expected_sheets && expected_sheets > 0;
    let rows_present = parses && total_rows >= 1;
    let checks = vec![
        check(
            "OFFICE_CSV_OPENS",
            status(parses),
            true,
            json!("every sheet CSV re-parses"),
            json!(if parses {
                format!("{observed_sheets} sheets parsed")
            } else {
                parse_failures.join("; ")
            }),
            "Lenient CSV reader re-opened every sheet export.",
        ),
        check(
            "OFFICE_CSV_SHEET_COUNT",
            status(sheet_count_ok),
            true,
            json!(expected_sheets),
            json!(observed_sheets),
            "Workbook worksheet count equals produced CSV files.",
        ),
        check(
            "OFFICE_CSV_ROWS_PRESENT",
            status(rows_present),
            true,
            json!(">= 1 row across all sheets"),
            json!(total_rows),
            "Row inventory across the exported worksheets.",
        ),
    ];
    let report_status = checks
        .iter()
        .fold(ValidationStatus::Pass, |current, check| {
            current.worst(check.status)
        });
    ValidationReport {
        schema_version: SCHEMA_VERSION,
        report_id: Uuid::new_v4(),
        job_id,
        plan_hash: plan.plan_hash.clone(),
        status: report_status,
        input: artifact_summary(input),
        output: directory_artifact_summary(output_dir, observed_sheets),
        engines: plan.steps.iter().map(|step| step.engine.clone()).collect(),
        checks,
        intentional_changes: plan.changes.changed.clone(),
        redaction: ReportRedaction {
            paths_redacted: false,
            metadata_values_redacted: true,
        },
    }
}

/// 目录输出的 artifact 摘要：不做指纹（目录不是单文件），格式 id 标记
/// csv-sheets 便于 UI 区分。
fn directory_artifact_summary(output_dir: &Path, sheet_count: usize) -> ArtifactSummary {
    let size_bytes = std::fs::read_dir(output_dir)
        .map(|directory| {
            directory
                .flatten()
                .filter_map(|entry| entry.metadata().ok())
                .map(|metadata| metadata.len())
                .sum()
        })
        .unwrap_or_default();
    ArtifactSummary {
        display_path: Some(output_dir.to_string_lossy().into_owned()),
        format_id: "csv-sheets".to_owned(),
        size_bytes,
        fast_fingerprint: format!("{sheet_count} sheets"),
        full_blake3: None,
    }
}

/// 宽松 CSV 计数：无表头语义、允许行间列数差异。返回 (行数, 最宽行字段数)。
fn count_csv_rows(path: &Path) -> std::result::Result<(usize, usize), String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .map_err(|error| error.to_string())?;
    let mut rows = 0_usize;
    let mut max_fields = 0_usize;
    for record in reader.records() {
        let record = record.map_err(|error| error.to_string())?;
        rows += 1;
        max_fields = max_fields.max(record.len());
    }
    Ok((rows, max_fields))
}

/// Structural validation for DOCX/ODF exchange output: the package must open
/// as a ZIP, present its container-specific required parts, and be detected
/// as the requested target family. 不依赖 Poppler（PDF 专用工具链）。
pub(crate) fn validate_office_document_output(
    input: &Probe,
    output: &Probe,
    plan: &Plan,
    job_id: Uuid,
) -> ValidationReport {
    let target = plan.target_format.as_str();
    let structure_ok = office_document_structure(&output.artifact.canonical_path, target);
    let checks = vec![
        check(
            "OFFICE_DOCUMENT_OPENS",
            status(output.format.id == target),
            true,
            json!(target),
            json!(output.format.id),
            "Native office inspector re-opened the converted package.",
        ),
        check(
            "OFFICE_TARGET_STRUCTURE",
            status(structure_ok),
            true,
            json!("required container parts present"),
            json!(structure_ok),
            "ZIP container carries the required OPC/ODF parts.",
        ),
        check(
            "OFFICE_VISUAL_DRIFT",
            ValidationStatus::Unknown,
            false,
            json!("fixture-calibrated structural comparison"),
            json!("not-run"),
            "Alpha validation checks container structure without a style baseline.",
        ),
    ];
    let hard_status = checks
        .iter()
        .filter(|check| check.required)
        .fold(ValidationStatus::Pass, |state, check| {
            state.worst(check.status)
        });
    let report_status = if hard_status == ValidationStatus::Pass {
        ValidationStatus::Warning
    } else {
        hard_status
    };
    ValidationReport {
        schema_version: SCHEMA_VERSION,
        report_id: Uuid::new_v4(),
        job_id,
        plan_hash: plan.plan_hash.clone(),
        status: report_status,
        input: artifact_summary(input),
        output: artifact_summary(output),
        engines: plan.steps.iter().map(|step| step.engine.clone()).collect(),
        checks,
        intentional_changes: plan.changes.changed.clone(),
        redaction: ReportRedaction {
            paths_redacted: false,
            metadata_values_redacted: true,
        },
    }
}

/// OPC 需要 `[Content_Types].xml` 与 `word/document.xml`；ODF 需要未压缩的
/// `mimetype` 与 `content.xml`。
fn office_document_structure(path: &Path, target: &str) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let Ok(archive) = ZipArchive::new(file) else {
        return false;
    };
    match target {
        "docx" => {
            archive.index_for_name("[Content_Types].xml").is_some()
                && archive.index_for_name("word/document.xml").is_some()
        }
        "odt" => {
            archive.index_for_name("mimetype").is_some()
                && archive.index_for_name("content.xml").is_some()
        }
        _ => false,
    }
}

/// Plans an image -> PDF page composition through `LibreOffice` Draw (`draw_pdf_Export`).
///
/// # Errors
///
/// Returns `Unsupported` for anything but PNG/JPEG input.
#[allow(clippy::too_many_lines)]
pub fn plan_image_to_pdf(
    probe: &Probe,
    output_path: std::path::PathBuf,
    soffice: &EngineIdentity,
    pdfinfo: &EngineIdentity,
    pdftoppm: &EngineIdentity,
) -> Result<Plan> {
    if !matches!(probe.format.id.as_str(), "png" | "jpeg" | "tiff" | "bmp") {
        return Err(unsupported(
            "Image-to-PDF accepts PNG, JPEG, TIFF, or BMP input",
        ));
    }
    if soffice.engine_id != "soffice"
        || pdfinfo.engine_id != "pdfinfo"
        || pdftoppm.engine_id != "pdftoppm"
    {
        return Err(AnoleError::new(
            ErrorCode::EngineIncompatible,
            Stage::Plan,
            "Image-to-PDF Plan was given an incorrect engine",
            "Run doctor and use soffice, pdfinfo, and pdftoppm.",
        ));
    }
    let conversion = PlanStep {
        step_id: "step-1".to_owned(),
        capability_id: format!("libreoffice.{}-to-pdf.headless", probe.format.id),
        engine: soffice.clone(),
        operation: Operation::Render,
        loss_class: LossClass::None,
        arguments: BTreeMap::from([
            ("source_format".to_owned(), probe.format.id.clone()),
            ("target_format".to_owned(), "pdf".to_owned()),
            ("headless".to_owned(), "true".to_owned()),
            ("isolated_profile".to_owned(), "true".to_owned()),
            ("macros".to_owned(), "disabled".to_owned()),
            ("external_resources".to_owned(), "deny".to_owned()),
        ]),
        estimated_temporary_bytes: Some(probe.artifact.size_bytes.saturating_mul(4)),
    };
    let structural_validation = PlanStep {
        step_id: "step-2".to_owned(),
        capability_id: "poppler.pdf-structural-validation.all-pages".to_owned(),
        engine: pdfinfo.clone(),
        operation: Operation::Inspect,
        loss_class: LossClass::None,
        arguments: BTreeMap::from([
            ("page_sizes".to_owned(), "required".to_owned()),
            ("target_format".to_owned(), "pdf".to_owned()),
            ("purpose".to_owned(), "validation-only".to_owned()),
        ]),
        estimated_temporary_bytes: None,
    };
    let render_validation = PlanStep {
        step_id: "step-3".to_owned(),
        capability_id: "poppler.pdf-render-validation.all-pages".to_owned(),
        engine: pdftoppm.clone(),
        operation: Operation::Inspect,
        loss_class: LossClass::None,
        arguments: BTreeMap::from([
            ("dpi".to_owned(), "72".to_owned()),
            ("target_format".to_owned(), "png".to_owned()),
            ("purpose".to_owned(), "validation-only".to_owned()),
        ]),
        estimated_temporary_bytes: None,
    };
    let mut plan = Plan {
        schema_version: SCHEMA_VERSION,
        plan_id: Uuid::new_v4(),
        plan_hash: String::new(),
        input_fingerprint: probe.artifact.fast_fingerprint.clone(),
        target_format: "pdf".to_owned(),
        constraints: BTreeMap::from([
            ("network".to_owned(), json!("deny")),
            ("external_resources".to_owned(), json!("deny")),
        ]),
        steps: vec![conversion, structural_validation, render_validation],
        changes: ChangeSet {
            preserved: vec!["image pixel data".to_owned()],
            changed: vec!["the image is placed on a PDF page".to_owned()],
            dropped: vec![],
            unknown: vec!["embedded image is not text-searchable without OCR".to_owned()],
        },
        validators: vec!["office.page-render".to_owned()],
        network_policy: NetworkPolicy::Deny,
        output_path: Some(output_path),
        estimated_output_bytes: None,
    };
    plan.plan_hash = deterministic_plan_hash(&plan)?;
    Ok(plan)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn validate_office_pdf_output(
    input: &Probe,
    output: &Probe,
    plan: &Plan,
    job_id: Uuid,
    rendered_page_count: usize,
    engine_diagnostic: &str,
) -> ValidationReport {
    let page_count = output
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Page)
        .count();
    let page_sizes_valid = output.streams.iter().all(|stream| {
        stream
            .properties
            .get("width_points")
            .and_then(Value::as_f64)
            .is_some_and(|value| value > 0.0)
            && stream
                .properties
                .get("height_points")
                .and_then(Value::as_f64)
                .is_some_and(|value| value > 0.0)
    });
    let lower_diagnostic = engine_diagnostic.to_ascii_lowercase();
    let font_warning = ["font", "substitut", "glyph"]
        .iter()
        .any(|needle| lower_diagnostic.contains(needle));
    let checks = vec![
        check(
            "OFFICE_PDF_OPENS",
            status(output.format.id == "pdf"),
            true,
            json!("pdf"),
            json!(output.format.id),
            "pdfinfo independently opened the staged output.",
        ),
        check(
            "OFFICE_PDF_PAGE_COUNT",
            status(page_count > 0),
            true,
            json!(">=1"),
            json!(page_count),
            "Ordered page streams reported by pdfinfo.",
        ),
        check(
            "OFFICE_PDF_PAGE_SIZES",
            status(page_sizes_valid),
            true,
            json!("positive point dimensions for every page"),
            json!(page_sizes_valid),
            "Per-page PDF size metadata.",
        ),
        check(
            "OFFICE_PDF_ALL_PAGES_RENDER",
            status(rendered_page_count == page_count && page_count > 0),
            true,
            json!(page_count),
            json!(rendered_page_count),
            "pdftoppm rendered every page and native PNG decoding opened each render.",
        ),
        check(
            "OFFICE_FONT_DIAGNOSTICS",
            if font_warning {
                ValidationStatus::Warning
            } else {
                ValidationStatus::Pass
            },
            false,
            json!("no engine-reported font warning"),
            json!(if font_warning {
                "warning-detected"
            } else {
                "none-detected"
            }),
            "Bounded LibreOffice diagnostic; absence is not proof of font identity.",
        ),
        check(
            "OFFICE_VISUAL_DRIFT",
            ValidationStatus::Unknown,
            false,
            json!("fixture-calibrated visual comparison"),
            json!("not-run"),
            "Alpha validation renders all pages without a source-reference baseline.",
        ),
    ];
    let hard_status = checks
        .iter()
        .filter(|check| check.required)
        .fold(ValidationStatus::Pass, |state, check| {
            state.worst(check.status)
        });
    let report_status = if hard_status == ValidationStatus::Pass {
        ValidationStatus::Warning
    } else {
        hard_status
    };
    ValidationReport {
        schema_version: SCHEMA_VERSION,
        report_id: Uuid::new_v4(),
        job_id,
        plan_hash: plan.plan_hash.clone(),
        status: report_status,
        input: artifact_summary(input),
        output: artifact_summary(output),
        engines: plan.steps.iter().map(|step| step.engine.clone()).collect(),
        checks,
        intentional_changes: plan.changes.changed.clone(),
        redaction: ReportRedaction {
            paths_redacted: false,
            metadata_values_redacted: true,
        },
    }
}

#[derive(Debug)]
struct PackageInspection {
    format: &'static str,
    entry_count: usize,
    expanded_bytes: u64,
    has_macros: bool,
    has_external_relationships: bool,
}

/// RTF is a plain-text envelope, not a ZIP package: no parts to enumerate,
/// and the alpha policy relies on `LibreOffice`'s own parser rather than deep
/// RTF inspection.
fn inspect_rtf_envelope(path: &Path) -> PackageInspection {
    PackageInspection {
        format: "rtf",
        entry_count: 1,
        expanded_bytes: std::fs::metadata(path).map_or(0, |meta| meta.len()),
        has_macros: false,
        has_external_relationships: false,
    }
}

fn inspect_package(path: &Path) -> Result<PackageInspection> {
    let file = File::open(path).map_err(|error| input_error(path, &error))?;
    let mut archive = ZipArchive::new(file).map_err(|error| invalid_zip(&error))?;
    let expanded_bytes = check_package_limits(&mut archive)?;
    let format = detect_package_family(&mut archive).ok_or_else(|| {
        AnoleError::new(
            ErrorCode::Unsupported,
            Stage::Inspect,
            "ZIP package is not recognized as DOCX, PPTX, or XLSX",
            "Choose a supported macro-free OOXML file.",
        )
    })?;
    let mut has_macros = false;
    let mut has_external_relationships = false;
    let mut relationship_bytes = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| package_read_error(&error))?;
        let name = entry.name().replace('\\', "/").to_ascii_lowercase();
        has_macros |= name.ends_with("vbaproject.bin");
        // ODF stores Basic macro sources under a `Basic/` member folder,
        // either at the package root or nested inside a subdirectory.
        has_macros |= name.starts_with("basic/") || name.contains("/basic/");
        if is_relationship_part(&name) {
            relationship_bytes = relationship_bytes.saturating_add(entry.size());
            if relationship_bytes > MAX_RELATIONSHIP_BYTES {
                return Err(resource_error(
                    "Office relationship XML exceeds the 8 MiB alpha limit",
                ));
            }
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|error| package_io_error(&error))?;
            has_external_relationships |= relationships_are_external(&bytes)?;
        }
    }
    Ok(PackageInspection {
        format,
        entry_count: archive.len(),
        expanded_bytes,
        has_macros,
        has_external_relationships,
    })
}

fn check_package_limits<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<u64> {
    if archive.len() > MAX_OFFICE_ENTRIES {
        return Err(resource_error(
            "Office package contains too many ZIP entries",
        ));
    }
    let mut expanded = 0_u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| package_read_error(&error))?;
        expanded = expanded.saturating_add(entry.size());
        if expanded > MAX_OFFICE_EXPANDED_BYTES {
            return Err(resource_error(
                "Office package expanded size exceeds the 1 GiB alpha limit",
            ));
        }
    }
    Ok(expanded)
}

fn detect_package_family<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Option<&'static str> {
    if archive.by_name("word/document.xml").is_ok() {
        Some("docx")
    } else if archive.by_name("ppt/presentation.xml").is_ok() {
        Some("pptx")
    } else if archive.by_name("xl/workbook.xml").is_ok() {
        Some("xlsx")
    } else if archive.by_name("content.xml").is_ok()
        && archive.by_name("META-INF/manifest.xml").is_ok()
    {
        // ODF packages carry their exact flavor in the uncompressed
        // `mimetype` entry; default to text when it cannot be read.
        let flavor = archive
            .by_name("mimetype")
            .ok()
            .and_then(|mut entry| {
                let mut buffer = String::new();
                entry.read_to_string(&mut buffer).ok().map(|_| buffer)
            })
            .unwrap_or_default();
        if flavor.contains("presentation") {
            Some("odp")
        } else if flavor.contains("spreadsheet") {
            Some("ods")
        } else {
            Some("odt")
        }
    } else {
        None
    }
}

fn relationships_are_external(bytes: &[u8]) -> Result<bool> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event) | Event::Empty(event)) => {
                for attribute in event.attributes().with_checks(true) {
                    let attribute = attribute.map_err(|error| relationship_xml_error(&error))?;
                    if attribute.key.local_name().as_ref() == b"TargetMode"
                        && attribute
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|error| relationship_xml_error(&error))?
                            .eq_ignore_ascii_case("external")
                    {
                        return Ok(true);
                    }
                }
            }
            Ok(Event::DocType(_)) => {
                return Err(AnoleError::new(
                    ErrorCode::PolicyBlocked,
                    Stage::Inspect,
                    "Office relationship XML contains a DTD",
                    "Remove active or external package content and retry.",
                ));
            }
            Ok(Event::Eof) => return Ok(false),
            Ok(_) => {}
            Err(error) => return Err(relationship_xml_error(&error)),
        }
    }
}

fn mime_type(format: &str) -> &'static str {
    match format {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "rtf" => "application/rtf",
        _ => "application/octet-stream",
    }
}

fn artifact_summary(probe: &Probe) -> ArtifactSummary {
    ArtifactSummary {
        display_path: Some(probe.artifact.display_path.clone()),
        format_id: probe.format.id.clone(),
        size_bytes: probe.artifact.size_bytes,
        fast_fingerprint: probe.artifact.fast_fingerprint.clone(),
        full_blake3: probe.artifact.full_blake3.clone(),
    }
}

fn check(
    code: &str,
    status: ValidationStatus,
    required: bool,
    expected: Value,
    observed: Value,
    evidence: &str,
) -> ValidationCheck {
    ValidationCheck {
        code: code.to_owned(),
        status,
        required,
        expected,
        observed,
        evidence: evidence.to_owned(),
        message: if status == ValidationStatus::Pass {
            "Office-to-PDF validation check passed.".to_owned()
        } else {
            "Office-to-PDF validation needs attention.".to_owned()
        },
    }
}

const fn status(condition: bool) -> ValidationStatus {
    if condition {
        ValidationStatus::Pass
    } else {
        ValidationStatus::Fail
    }
}

fn is_relationship_part(name: &str) -> bool {
    name.rsplit('/').next().is_some_and(|file_name| {
        file_name.eq_ignore_ascii_case(".rels")
            || Path::new(file_name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rels"))
    })
}

fn input_error(path: &Path, error: &std::io::Error) -> AnoleError {
    AnoleError::new(
        ErrorCode::InputInvalid,
        Stage::Inspect,
        format!("Unable to read Office input: {}", path.display()),
        "Check file permissions and storage health.",
    )
    .with_diagnostic(error.to_string())
}

fn invalid_zip(error: &zip::result::ZipError) -> AnoleError {
    AnoleError::new(
        ErrorCode::InputInvalid,
        Stage::Inspect,
        "Office package ZIP structure is invalid",
        "Choose a complete DOCX, PPTX, or XLSX file.",
    )
    .with_diagnostic(error.to_string())
}

fn resource_error(message: &str) -> AnoleError {
    AnoleError::new(
        ErrorCode::ResourceExhausted,
        Stage::Inspect,
        message,
        "Reduce or split the Office document, then retry.",
    )
}

fn package_read_error(error: &zip::result::ZipError) -> AnoleError {
    AnoleError::new(
        ErrorCode::InputInvalid,
        Stage::Inspect,
        "Unable to read an Office package entry",
        "Choose a complete OOXML document.",
    )
    .with_diagnostic(error.to_string())
}

fn package_io_error(error: &std::io::Error) -> AnoleError {
    AnoleError::new(
        ErrorCode::InputInvalid,
        Stage::Inspect,
        "Unable to read Office relationship XML",
        "Choose a complete OOXML document.",
    )
    .with_diagnostic(error.to_string())
}

fn relationship_xml_error(error: &impl std::fmt::Display) -> AnoleError {
    AnoleError::new(
        ErrorCode::InputInvalid,
        Stage::Inspect,
        "Office relationship XML is malformed",
        "Repair or recreate the Office document.",
    )
    .with_diagnostic(error.to_string())
}

fn unsupported(message: &str) -> AnoleError {
    AnoleError::new(
        ErrorCode::Unsupported,
        Stage::Plan,
        message,
        "Choose DOCX, PPTX, or XLSX input and PDF output.",
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use anole_engine_sdk::{Certification, EngineIdentity};
    use tempfile::NamedTempFile;
    use zip::write::SimpleFileOptions;

    use super::{inspect_office, office_format_hint, plan_office_to_pdf};

    fn write_odf(path: &std::path::Path, flavor: &str) {
        let file = std::fs::File::create(path).expect("create odf");
        let mut archive = zip::ZipWriter::new(file);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        archive.start_file("mimetype", stored).expect("mimetype");
        archive
            .write_all(flavor.as_bytes())
            .expect("mimetype payload");
        archive
            .start_file("META-INF/manifest.xml", SimpleFileOptions::default())
            .expect("manifest");
        archive
            .write_all(b"<?xml version=\"1.0\"?><manifest/>")
            .expect("manifest payload");
        archive
            .start_file("content.xml", SimpleFileOptions::default())
            .expect("content");
        archive
            .write_all(b"<?xml version=\"1.0\"?><office:document-content/>")
            .expect("content payload");
        archive.finish().expect("odf finish");
    }

    #[tokio::test]
    async fn odf_packages_are_detected_by_flavor() {
        let directory = tempfile::tempdir().expect("tempdir");
        let cases = [
            (
                "letter.odt",
                "application/vnd.oasis.opendocument.text",
                "odt",
            ),
            (
                "sheet.ods",
                "application/vnd.oasis.opendocument.spreadsheet",
                "ods",
            ),
            (
                "deck.odp",
                "application/vnd.oasis.opendocument.presentation",
                "odp",
            ),
        ];
        for (name, flavor, expected) in cases {
            let path = directory.path().join(name);
            write_odf(&path, flavor);
            let probe = inspect_office(&path).await.expect("odf inspection");
            assert_eq!(probe.format.id, expected, "{name} flavor detection");
            assert_eq!(probe.format.container.as_deref(), Some("zip/odf"));
        }
    }

    #[tokio::test]
    async fn odf_macro_folders_are_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("macro.odt");
        // A complete ODF package that additionally carries a Basic macro
        // member - the macro must dominate over the otherwise-valid parts.
        let file = std::fs::File::create(&path).expect("create macro odf");
        let mut archive = zip::ZipWriter::new(file);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        archive.start_file("mimetype", stored).expect("mimetype");
        archive
            .write_all(b"application/vnd.oasis.opendocument.text")
            .expect("mimetype payload");
        archive
            .start_file("META-INF/manifest.xml", SimpleFileOptions::default())
            .expect("manifest");
        archive
            .write_all(b"<?xml version=\"1.0\"?><manifest/>")
            .expect("manifest payload");
        archive
            .start_file("content.xml", SimpleFileOptions::default())
            .expect("content");
        archive
            .write_all(b"<?xml version=\"1.0\"?><office:document-content/>")
            .expect("content payload");
        archive
            .start_file("Basic/Module1.xml", SimpleFileOptions::default())
            .expect("macro member");
        archive.write_all(b"<basic/>").expect("macro payload");
        archive.finish().expect("finish");
        let error = inspect_office(&path)
            .await
            .expect_err("macro ODF must be blocked");
        assert_eq!(error.code, crate::ErrorCode::PolicyBlocked);
    }

    #[tokio::test]
    async fn document_exchange_plans_docx_odt_in_both_directions() {
        let directory = tempfile::tempdir().expect("tempdir");
        let docx = directory.path().join("letter.docx");
        write_docx(&docx);
        let probe = inspect_office(&docx).await.expect("docx inspection");
        let plan = super::plan_office_document_exchange(
            &probe,
            PathBuf::from("out.odt"),
            &engine("soffice"),
            "odt",
        )
        .expect("docx -> odt plan");
        assert_eq!(plan.target_format, "odt");
        assert_eq!(
            plan.steps[0]
                .arguments
                .get("source_format")
                .map(String::as_str),
            Some("docx")
        );
        assert!(
            plan.validators
                .contains(&"office.target-structure".to_owned())
        );
        assert!(!plan.plan_hash.is_empty());

        let odt = directory.path().join("letter.odt");
        write_odf(&odt, "application/vnd.oasis.opendocument.text");
        let odt_probe = inspect_office(&odt).await.expect("odt inspection");
        let plan = super::plan_office_document_exchange(
            &odt_probe,
            PathBuf::from("out.docx"),
            &engine("soffice"),
            "docx",
        )
        .expect("odt -> docx plan");
        assert_eq!(plan.target_format, "docx");

        // 同格式、非法目标与错误引擎都要被拒绝。
        assert!(
            super::plan_office_document_exchange(
                &probe,
                PathBuf::from("out.docx"),
                &engine("soffice"),
                "docx"
            )
            .is_err(),
            "docx -> docx is rejected"
        );
        assert!(
            super::plan_office_document_exchange(
                &probe,
                PathBuf::from("out.pdf"),
                &engine("soffice"),
                "pdf"
            )
            .is_err(),
            "pdf target is rejected"
        );
        assert!(
            super::plan_office_document_exchange(
                &probe,
                PathBuf::from("out.odt"),
                &engine("pandoc"),
                "odt"
            )
            .is_err(),
            "non-soffice engine is rejected"
        );
    }

    #[tokio::test]
    async fn document_output_structure_validation_checks_container_parts() {
        let directory = tempfile::tempdir().expect("tempdir");
        let docx = directory.path().join("letter.docx");
        write_docx(&docx);
        let input_probe = inspect_office(&docx).await.expect("docx inspection");
        let plan = super::plan_office_document_exchange(
            &input_probe,
            directory.path().join("out.odt"),
            &engine("soffice"),
            "odt",
        )
        .expect("exchange plan");

        // 一个合法 ODF 输出 → 结构检查通过；一个缺 content.xml 的 ZIP → Fail。
        let valid = directory.path().join("converted.odt");
        write_odf(&valid, "application/vnd.oasis.opendocument.text");
        let valid_probe = inspect_office(&valid).await.expect("odt output inspection");
        let report = super::validate_office_document_output(
            &input_probe,
            &valid_probe,
            &plan,
            uuid::Uuid::new_v4(),
        );
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.code == "OFFICE_TARGET_STRUCTURE"
                    && check.status == crate::domain::ValidationStatus::Pass)
        );

        let broken = directory.path().join("broken.odt");
        let file = std::fs::File::create(&broken).expect("create broken odt");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("mimetype", stored_options())
            .expect("mimetype");
        std::io::Write::write_all(&mut writer, b"application/vnd.oasis.opendocument.text")
            .expect("mimetype payload");
        writer.finish().expect("finish broken");
        let error = inspect_office(&broken)
            .await
            .expect_err("incomplete ODF cannot be probed as odt");
        assert_eq!(error.code, crate::ErrorCode::Unsupported);
    }

    fn stored_options() -> zip::write::SimpleFileOptions {
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
    }

    fn write_docx(path: &std::path::Path) {
        let file = std::fs::File::create(path).expect("create docx");
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        writer
            .start_file("[Content_Types].xml", options)
            .expect("content types");
        std::io::Write::write_all(&mut writer, b"<Types/>").expect("content types XML");
        writer.start_file("_rels/.rels", options).expect("rels");
        std::io::Write::write_all(&mut writer, b"<Relationships/>").expect("rels XML");
        writer
            .start_file("word/document.xml", options)
            .expect("document part");
        std::io::Write::write_all(&mut writer, b"<w:document/>").expect("document XML");
        writer.finish().expect("finish docx");
    }

    #[tokio::test]
    async fn rtf_envelopes_inspect_without_a_zip_package() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("note.rtf");
        std::fs::write(&path, b"{\\rtf1\\ansi ELECTRIC 440 cell}").expect("write rtf");
        let probe = inspect_office(&path).await.expect("rtf inspection");
        assert_eq!(probe.format.id, "rtf");
        assert_eq!(probe.format.mime_type.as_deref(), Some("application/rtf"));
        let plan = plan_office_to_pdf(
            &probe,
            PathBuf::from("out.pdf"),
            &engine("soffice"),
            &engine("pdfinfo"),
            &engine("pdftoppm"),
        );
        assert!(plan.is_ok(), "RTF plans through the office lane");
    }

    fn write_xlsx(path: &std::path::Path) {
        let file = std::fs::File::create(path).expect("create xlsx");
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        writer
            .start_file("[Content_Types].xml", options)
            .expect("content types");
        std::io::Write::write_all(&mut writer, b"<Types/>").expect("content types XML");
        writer.start_file("_rels/.rels", options).expect("rels");
        std::io::Write::write_all(&mut writer, b"<Relationships/>").expect("rels XML");
        writer
            .start_file("xl/workbook.xml", options)
            .expect("workbook part");
        std::io::Write::write_all(&mut writer, b"<workbook/>").expect("workbook XML");
        writer.finish().expect("finish xlsx");
    }

    #[tokio::test]
    async fn xlsx_csv_export_plan_declares_all_worksheet_semantics() {
        let directory = tempfile::tempdir().expect("tempdir");
        let xlsx = directory.path().join("budget.xlsx");
        write_xlsx(&xlsx);
        let probe = inspect_office(&xlsx).await.expect("xlsx inspection");
        assert_eq!(probe.format.id, "xlsx");
        let plan = super::plan_office_csv_export(
            &probe,
            directory.path().join("out-sheets"),
            &engine("anole.office-csv"),
        )
        .expect("csv export plan");
        assert_eq!(plan.target_format, "csv");
        assert_eq!(
            plan.steps[0]
                .arguments
                .get("worksheets")
                .map(String::as_str),
            Some("all")
        );
        assert!(
            plan.changes
                .changed
                .iter()
                .any(|item| item.contains("each worksheet")),
            "per-sheet output is declared honestly"
        );
        assert!(
            plan.steps[0].engine.engine_id == "anole.office-csv",
            "the lane is the built-in engine, not soffice"
        );
        // 非 xlsx 输入与错误引擎都要拒绝。
        let docx = directory.path().join("letter.docx");
        write_docx(&docx);
        let docx_probe = inspect_office(&docx).await.expect("docx inspection");
        assert!(
            super::plan_office_csv_export(
                &docx_probe,
                PathBuf::from("out-sheets"),
                &engine("anole.office-csv")
            )
            .is_err()
        );
        assert!(
            super::plan_office_csv_export(&probe, PathBuf::from("out-sheets"), &engine("soffice"))
                .is_err()
        );
    }

    #[test]
    fn sheet_suffix_sanitizes_cross_system_characters() {
        assert_eq!(super::sanitize_sheet_suffix("Data"), "-Data");
        assert_eq!(
            super::sanitize_sheet_suffix("2024 年/预算"),
            "-2024_年_预算"
        );
        assert_eq!(super::sanitize_sheet_suffix("电压表"), "-电压表");
        assert_eq!(super::sanitize_sheet_suffix("___"), "-___");
        assert_eq!(super::sanitize_sheet_suffix(""), "");
        let long = "x".repeat(80);
        assert_eq!(
            super::sanitize_sheet_suffix(&long).chars().count(),
            1 + 32,
            "suffix is capped at 32 characters plus the dash"
        );
    }

    fn engine(id: &str) -> EngineIdentity {
        EngineIdentity {
            engine_id: id.to_owned(),
            version: "test".to_owned(),
            binary_path: PathBuf::from(id),
            binary_sha256: "sha".to_owned(),
            manifest_sha256: None,
            build_configuration: None,
            certification: Certification::Experimental,
        }
    }

    fn package(required_part: &str, relationships: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("temporary package");
        {
            let mut writer = zip::ZipWriter::new(file.as_file_mut());
            let options = SimpleFileOptions::default();
            writer
                .start_file("[Content_Types].xml", options)
                .expect("content types");
            writer.write_all(b"<Types/>").expect("content types XML");
            writer
                .start_file(required_part, options)
                .expect("required part");
            writer.write_all(b"<root/>").expect("required XML");
            writer
                .start_file("_rels/.rels", options)
                .expect("relationships");
            writer
                .write_all(relationships.as_bytes())
                .expect("relationship XML");
            writer.finish().expect("finish package");
        }
        file
    }

    #[tokio::test]
    async fn detects_docx_from_package_parts_and_plans_isolation() {
        let file = package(
            "word/document.xml",
            r#"<Relationships><Relationship TargetMode="Internal"/></Relationships>"#,
        );
        assert_eq!(office_format_hint(file.path()).expect("hint"), Some("docx"));
        let probe = inspect_office(file.path()).await.expect("office probe");
        let plan = plan_office_to_pdf(
            &probe,
            PathBuf::from("output.pdf"),
            &engine("soffice"),
            &engine("pdfinfo"),
            &engine("pdftoppm"),
        )
        .expect("office Plan");
        assert_eq!(plan.target_format, "pdf");
        assert_eq!(plan.constraints["isolated_user_profile"], true);
        assert_eq!(plan.steps.len(), 3);
    }

    #[tokio::test]
    async fn blocks_external_relationships() {
        let file = package(
            "ppt/presentation.xml",
            r#"<Relationships><Relationship TargetMode="External" Target="https://example.invalid/"/></Relationships>"#,
        );
        let error = inspect_office(file.path())
            .await
            .expect_err("external link blocked");
        assert_eq!(error.code, crate::ErrorCode::PolicyBlocked);
    }
}
