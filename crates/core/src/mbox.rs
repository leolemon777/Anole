//! MBOX 邮箱聚合输入：解析拆分为逐封 EML，txt/html 直接聚合渲染，
//! pdf 则逐封渲染→逐封 PDF→qpdf 合并，实现「整个邮箱导出一个 PDF」。
//! 内置 `anole.mbox` 引擎；解析失败 fail-closed。
//!
//! 变体策略：带 `Content-Length` 头的文件按 mboxcl 精确切分（声明与
//! 实际不符时拒绝）；无该头时无法区分 mboxrd/mboxo，保守按 mboxo
//! 解析——`>From ` 行原样保留（漏转义只多一个 `>`，错误 unescape 会
//! 删字符破坏正文），假设记录进 Probe 属性。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anole_engine_sdk::{EngineIdentity, LossClass, Operation};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::domain::{
    ArtifactSummary, ChangeSet, FormatDescriptor, FormatKind, NetworkPolicy, Plan, PlanRequest,
    PlanStep, Probe, ProbeEvidence, ReportRedaction, SCHEMA_VERSION, StreamKind, StreamProbe,
    ValidationCheck, ValidationReport, ValidationStatus,
};
use crate::eml::{self, ParsedEmail};
use crate::error::{AnoleError, ErrorCode, Result, Stage};
use crate::fingerprint::identify_artifact;
use crate::planner::deterministic_plan_hash;

pub const MBOX_ENGINE_ID: &str = "anole.mbox";

const MAX_MBOX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MBOX_MAILS: usize = 1000;

/// 一封从 MBOX 拆出的邮件：原始 EML 字节 + 解析结果。
#[derive(Debug)]
pub struct MboxMail {
    pub eml_bytes: Vec<u8>,
    pub email: ParsedEmail,
}

/// MBOX 存储变体：决定切分与 `>From ` 转义还原策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MboxVariant {
    /// mboxcl：写入方附带 `Content-Length` 头（转义方案同 mboxrd），
    /// 读取时优先按该头精确切分，并还原一层转义。
    Cl,
    /// mboxo：写入方投递时不转义 `>From `；读取时按正文原样保留。
    O,
    /// mboxrd：写入方转义 `>*From `；读取时还原一层转义。
    Rd,
}

impl MboxVariant {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cl => "mboxcl",
            Self::O => "mboxo",
            Self::Rd => "mboxrd",
        }
    }

    /// 该变体下是否把 `>From ` 开头的行还原一层转义。
    fn unescapes(self) -> bool {
        matches!(self, Self::Cl | Self::Rd)
    }
}

/// 按行首 `From `（前面是文件头或空行）分界拆分 MBOX；变体自动检测：
/// 任一封带 `Content-Length` 头 → mboxcl，否则保守按 mboxo。
///
/// # Errors
///
/// `InputInvalid`：空邮箱、超限（字节或封数）、变体切分校验失败
/// （如 `Content-Length` 与实际正文不符）。
pub fn split_mbox_bytes(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    split_mbox_bytes_variant(bytes, None).map(|(_, messages)| messages)
}

/// 同 [`split_mbox_bytes`]，但可用 `requested` 显式声明变体；`None`
/// 时自动检测。返回实际使用的变体供 Probe 记录（assumed variant）。
///
/// - mboxcl：优先用 `Content-Length` 精确切分邮件体（正文中的 `From `
///   行属于正文）；无该头的邮件回退 From_ 启发式；读到的长度与实际
///   不符时 fail-closed，不静默截断。
/// - mboxo/mboxrd：From_ 启发式切分；前者不还原 `>From ` 转义（保守
///   假设），后者还原一层。
///
/// # Errors
///
/// 同 [`split_mbox_bytes`]。
pub fn split_mbox_bytes_variant(
    bytes: &[u8],
    requested: Option<MboxVariant>,
) -> Result<(MboxVariant, Vec<Vec<u8>>)> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Inspect,
            "MBOX input is not valid UTF-8",
            "Choose an mbox file exported in UTF-8.",
        )
        .with_diagnostic(error.to_string())
    })?;
    let (spans, saw_content_length) = split_into_spans(text)?;
    // mboxrd 与 mboxo 无法从文件本身可靠区分：默认按 mboxo 保守解析，
    // 漏转义（多一个 `>`）比错误 unescape（删字符）安全。
    let variant = requested.unwrap_or(if saw_content_length {
        MboxVariant::Cl
    } else {
        MboxVariant::O
    });
    let messages: Vec<Vec<u8>> = spans
        .iter()
        .map(|span| render_span(span, variant))
        .collect();
    if messages.is_empty() {
        return Err(AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Inspect,
            "The MBOX file contains no messages",
            "Choose an mbox file with at least one mail.",
        ));
    }
    if messages.len() > MAX_MBOX_MAILS {
        return Err(AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Inspect,
            format!(
                "The MBOX holds {} mails; the adapter limit is {MAX_MBOX_MAILS}",
                messages.len()
            ),
            "Split the mailbox before converting.",
        ));
    }
    Ok((variant, messages))
}

/// 单封邮件的切分结果（不含 envelope `From_` 行）。
enum MessageSpan<'a> {
    /// From_ 启发式切出：整封行（头部 + 空 + 正文）。
    Scanned { lines: Vec<&'a str> },
    /// `Content-Length` 精确定位：头部行 + 正文字节段。
    Measured { header: Vec<&'a str>, body: &'a str },
}

/// 行首字节偏移 + 剥掉 `\n`/`\r\n` 行尾的行文本。
struct LineSpan<'a> {
    start: usize,
    text: &'a str,
}

fn split_line_spans(text: &str) -> Vec<LineSpan<'_>> {
    let mut spans = Vec::new();
    let mut start = 0;
    for with_ending in text.split_inclusive('\n') {
        let text = match with_ending.strip_suffix('\n') {
            Some(line) => line.strip_suffix('\r').unwrap_or(line),
            None => with_ending,
        };
        spans.push(LineSpan { start, text });
        start += with_ending.len();
    }
    spans
}

/// 头部行里查找 `Content-Length`（头名大小写不敏感；无效值当作不存在）。
fn content_length_of(header: &[&str]) -> Option<usize> {
    header.iter().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    })
}

/// 逐封定位：有 `Content-Length` 的封按字节精切（并校验声明与实际相符），
/// 否则回退「空行 + 行首 `From `」启发式。返回切分结果与是否见过
/// `Content-Length` 头。
///
/// # Errors
///
/// `InputInvalid`：首行不是 `From_` envelope、`Content-Length` 超出
/// 文件或与其后的正文/下一封 envelope 不符（fail-closed，不静默截断）。
#[allow(clippy::too_many_lines)]
fn split_into_spans(text: &str) -> Result<(Vec<MessageSpan<'_>>, bool)> {
    let lines = split_line_spans(text);
    let envelope_error = || {
        AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Inspect,
            "The file does not start with an mbox \"From \" envelope line",
            "Choose a file in mbox/mboxo/mboxrd/mboxcl format.",
        )
    };
    let Some(first) = lines.iter().position(|line| !line.text.trim().is_empty()) else {
        return Err(envelope_error());
    };
    if !lines[first].text.starts_with("From ") {
        return Err(envelope_error());
    }
    // 启发式切点：行首 `From ` 且前面是文件头或空行。
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            line.text.starts_with("From ")
                && (*index == 0 || lines[index - 1].text.trim().is_empty())
        })
        .map(|(index, _)| index)
        .collect();
    let length_mismatch = |declared: usize| {
        AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Inspect,
            format!(
                "A mail declares Content-Length {declared} bytes but the stored body does not match"
            ),
            "Re-export the mailbox or repair the Content-Length header.",
        )
    };
    let mut spans = Vec::new();
    let mut saw_content_length = false;
    let mut hint = 0_usize;
    let mut cursor = first;
    loop {
        while hint < starts.len() && starts[hint] <= cursor {
            hint += 1;
        }
        let next_heuristic = starts.get(hint).copied();
        let limit = next_heuristic.unwrap_or(lines.len());
        let mut header_end = None;
        let mut index = cursor + 1;
        while index < limit {
            if lines[index].text.trim().is_empty() {
                header_end = Some(index);
                break;
            }
            index += 1;
        }
        let declared = header_end
            .map(|end| (cursor + 1..end).map(|j| lines[j].text).collect::<Vec<_>>())
            .and_then(|header| content_length_of(&header));
        if let (Some(end), Some(declared)) = (header_end, declared) {
            // mboxcl：正文按声明字节数精切，其中的 `From ` 行属于正文。
            saw_content_length = true;
            let body_start_line = end + 1;
            let body_start = if body_start_line < lines.len() {
                lines[body_start_line].start
            } else {
                text.len()
            };
            let body_end = body_start + declared;
            if body_end > text.len() {
                return Err(length_mismatch(declared));
            }
            // 正文之后：第一个非空行必须是下一封的 envelope From_ 行
            // （或到文件尾全空白），否则说明声明偏短截断了正文。
            let mut offset = 0_usize;
            let mut next_from: Option<usize> = None;
            for segment in text[body_end..].split_inclusive('\n') {
                if segment.trim().is_empty() {
                    offset += segment.len();
                    continue;
                }
                if !segment.trim_end_matches(['\n', '\r']).starts_with("From ") {
                    return Err(length_mismatch(declared));
                }
                let target = body_end + offset;
                let aligned = lines.partition_point(|line| line.start < target);
                if aligned >= lines.len()
                    || lines[aligned].start != target
                    || !lines[aligned].text.starts_with("From ")
                {
                    return Err(length_mismatch(declared));
                }
                next_from = Some(aligned);
                break;
            }
            spans.push(MessageSpan::Measured {
                header: (cursor + 1..end).map(|j| lines[j].text).collect(),
                body: &text[body_start..body_end],
            });
            match next_from {
                Some(next) => cursor = next,
                None => break,
            }
        } else {
            // From_ 启发式（mboxo/mboxrd 共用切分）。
            let mut collected = Vec::new();
            for line in &lines[cursor + 1..limit] {
                collected.push(line.text);
            }
            if collected.is_empty() {
                break;
            }
            spans.push(MessageSpan::Scanned { lines: collected });
            match next_heuristic {
                Some(next) => cursor = next,
                None => break,
            }
        }
    }
    Ok((spans, saw_content_length))
}

/// 按变体渲染一封邮件：行统一 `\r\n` 连接；mboxrd/mboxcl 把 `>From `
/// 开头的行还原一层转义，mboxo 保留原样。
fn render_span(span: &MessageSpan<'_>, variant: MboxVariant) -> Vec<u8> {
    let unescape = variant.unescapes();
    let lines: Vec<String> = match span {
        MessageSpan::Scanned { lines } => lines
            .iter()
            .map(|line| unescape_from_line(line, unescape))
            .collect(),
        MessageSpan::Measured { header, body } => header
            .iter()
            .map(|line| unescape_from_line(line, unescape))
            .chain(std::iter::once(String::new()))
            .chain(body.lines().map(|line| unescape_from_line(line, unescape)))
            .collect(),
    };
    lines.join("\r\n").into_bytes()
}

fn unescape_from_line(line: &str, enabled: bool) -> String {
    if enabled
        && let Some(rest) = line.strip_prefix('>')
        && rest.starts_with("From ")
    {
        rest.to_owned()
    } else {
        line.to_owned()
    }
}

/// 拆分并把每封解析为 [`ParsedEmail`]（任何一封失败即整体 fail-closed）。
///
/// # Errors
///
/// 同 [`split_mbox_bytes`]，外加逐封 EML 解析错误。
pub fn parse_mbox_file(path: &Path) -> Result<Vec<MboxMail>> {
    parse_mbox_file_variant(path, None).map(|(_, mails)| mails)
}

/// 同 [`parse_mbox_file`]，但可显式声明变体；返回实际使用的变体。
///
/// # Errors
///
/// 同 [`parse_mbox_file`]。
pub fn parse_mbox_file_variant(
    path: &Path,
    requested: Option<MboxVariant>,
) -> Result<(MboxVariant, Vec<MboxMail>)> {
    if let Ok(metadata) = std::fs::metadata(path)
        && metadata.len() > MAX_MBOX_BYTES
    {
        return Err(AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Inspect,
            "MBOX input exceeds the 256 MiB built-in adapter limit",
            "Split the mailbox before converting.",
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Inspect,
            "Unable to read the MBOX file",
            "Choose an existing mbox file.",
        )
        .with_diagnostic(error.to_string())
    })?;
    let (variant, messages) = split_mbox_bytes_variant(&bytes, requested)?;
    let mut mails = Vec::with_capacity(messages.len());
    for eml_bytes in messages {
        let email = eml::parse_eml_bytes(&eml_bytes)?;
        mails.push(MboxMail { eml_bytes, email });
    }
    Ok((variant, mails))
}

/// 构建 MBOX Probe：逐封聚合的属性挂在首条 stream 上（与 EML/MSG 同形）。
///
/// # Errors
///
/// 返回读取/解析错误。
pub async fn inspect_mbox(path: &Path) -> Result<Probe> {
    let artifact = identify_artifact(path).await?;
    let (variant, mails) = parse_mbox_file_variant(path, None)?;
    Ok(Probe {
        schema_version: SCHEMA_VERSION,
        artifact,
        format: FormatDescriptor {
            id: "mbox".to_owned(),
            kind: FormatKind::Document,
            mime_type: Some("application/mbox".to_owned()),
            container: Some(variant.as_str().to_owned()),
            extension_matches: Some(true),
            confidence: 1.0,
        },
        streams: vec![StreamProbe {
            index: 0,
            kind: StreamKind::Page,
            codec: None,
            language: None,
            duration_seconds: None,
            width: None,
            height: None,
            frame_rate: None,
            sample_rate: None,
            channels: None,
            properties: mbox_properties(&mails, variant),
        }],
        metadata: BTreeMap::new(),
        warnings: Vec::new(),
        evidence: ProbeEvidence {
            engine_id: MBOX_ENGINE_ID.to_owned(),
            engine_version: env!("CARGO_PKG_VERSION").to_owned(),
            engine_binary_sha256: None,
        },
        duration_seconds: None,
        bit_rate: None,
    })
}

fn mbox_properties(mails: &[MboxMail], variant: MboxVariant) -> BTreeMap<String, Value> {
    let concatenated = mails
        .iter()
        .map(|mail| mail.email.visible_text())
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = crate::document::normalized_tokens(&concatenated);
    let has_external_resource = mails.iter().any(|mail| {
        mail.email
            .html_body
            .as_deref()
            .is_some_and(crate::eml::contains_remote_reference)
    });
    let mut properties = BTreeMap::new();
    properties.insert("mail_count".to_owned(), json!(mails.len()));
    // 诚实披露变体判定依据：mboxcl 由 Content-Length 头检测，其余是
    // 保守假设（mboxrd/mboxo 无法从文件本身区分）。
    properties.insert("mbox_variant".to_owned(), json!(variant.as_str()));
    properties.insert(
        "variant_basis".to_owned(),
        json!(if variant == MboxVariant::Cl {
            "content-length-header"
        } else {
            "assumed"
        }),
    );
    if let Some(first) = mails.first()
        && let Some(value) = &first.email.subject
    {
        properties.insert("eml_subject".to_owned(), json!(value));
    }
    properties.insert(
        "semantic_token_digest".to_owned(),
        json!(format!(
            "blake3:{}",
            blake3::hash(normalized.as_bytes()).to_hex()
        )),
    );
    properties.insert(
        "text_characters".to_owned(),
        json!(normalized.chars().count()),
    );
    properties.insert(
        "has_external_resource".to_owned(),
        json!(has_external_resource),
    );
    properties
}

fn stream_property(probe: &Probe, name: &str) -> Value {
    probe
        .streams
        .first()
        .and_then(|stream| stream.properties.get(name))
        .cloned()
        .unwrap_or(Value::Null)
}

/// 逐封渲染的分隔标记；PDF 文本层回读时逐一核对，证明每封都进了合并。
fn mail_separator(index: usize, total: usize) -> String {
    format!("==== Anole Mail {}/{} ====", index + 1, total)
}

/// Builds the MBOX export Plan: txt/html are single builtin renders;
/// pdf is the composite (builtin render per mail → html→pdf lane per mail
/// → qpdf merge) whose per-mail sub-conversions resolve their own plans at
/// execution, exactly like chain segments.
///
/// # Errors
///
/// `Unsupported` for wrong input/target/engine, `PolicyBlocked` when any
/// mail's HTML references remote resources under deny-all.
#[allow(clippy::too_many_lines)]
pub fn plan_mbox_export(
    probe: &Probe,
    output_path: PathBuf,
    engine: &EngineIdentity,
    qpdf: &EngineIdentity,
    target: &str,
) -> Result<Plan> {
    let target = target.trim().trim_start_matches('.').to_ascii_lowercase();
    if probe.format.id != "mbox" {
        return Err(AnoleError::new(
            ErrorCode::Unsupported,
            Stage::Plan,
            "MBOX export input must be an mbox mailbox",
            "Choose a file in mbox/mboxo/mboxrd/mboxcl format.",
        ));
    }
    if !matches!(target.as_str(), "txt" | "html" | "pdf" | "md") {
        return Err(AnoleError::new(
            ErrorCode::Unsupported,
            Stage::Plan,
            "MBOX export target must be txt, html, md, or pdf",
            "Choose txt, html, md, or the whole-mailbox pdf.",
        ));
    }
    if engine.engine_id != MBOX_ENGINE_ID {
        return Err(AnoleError::new(
            ErrorCode::EngineIncompatible,
            Stage::Plan,
            "The MBOX export Plan was given the wrong engine",
            "Use the built-in anole.mbox adapter.",
        ));
    }
    if stream_property(probe, "has_external_resource") == json!(true) {
        return Err(AnoleError::new(
            ErrorCode::PolicyBlocked,
            Stage::Plan,
            "A mail in the MBOX references an external resource under deny-all policy",
            "Remove the remote image/link or wait for an explicitly authorized resource-root policy.",
        ));
    }
    let mail_count = stream_property(probe, "mail_count")
        .as_u64()
        .unwrap_or_default();
    let mut steps = vec![PlanStep {
        step_id: "step-1".to_owned(),
        capability_id: format!("anole.mbox.mbox-render-{target}.builtin"),
        engine: engine.clone(),
        operation: Operation::Transform,
        loss_class: LossClass::Unknown,
        arguments: BTreeMap::from([
            ("source_format".to_owned(), "mbox".to_owned()),
            ("target_format".to_owned(), target.clone()),
            ("mail_count".to_owned(), mail_count.to_string()),
            ("network".to_owned(), "deny".to_owned()),
            ("sanitize_html".to_owned(), "true".to_owned()),
        ]),
        estimated_temporary_bytes: Some(probe.artifact.size_bytes.saturating_mul(4)),
    }];
    let mut validators = vec![
        "document.text-extractable".to_owned(),
        "mbox.mail-separators".to_owned(),
    ];
    if target == "pdf" {
        if qpdf.engine_id != "qpdf" {
            return Err(AnoleError::new(
                ErrorCode::EngineIncompatible,
                Stage::Plan,
                "The MBOX merge step was given the wrong engine",
                "Run doctor and use qpdf.",
            ));
        }
        steps.push(PlanStep {
            step_id: "step-2".to_owned(),
            capability_id: "qpdf.mbox-merge.all-mails".to_owned(),
            engine: qpdf.clone(),
            operation: Operation::Transform,
            loss_class: LossClass::None,
            arguments: BTreeMap::from([
                ("merge_mode".to_owned(), "concatenate-1-z".to_owned()),
                ("mail_count".to_owned(), mail_count.to_string()),
            ]),
            estimated_temporary_bytes: Some(probe.artifact.size_bytes.saturating_mul(6)),
        });
        validators.push("mbox.page-conservation".to_owned());
    }
    let mut plan = Plan {
        schema_version: SCHEMA_VERSION,
        plan_id: Uuid::new_v4(),
        plan_hash: String::new(),
        input_fingerprint: probe.artifact.fast_fingerprint.clone(),
        target_format: target.clone(),
        constraints: BTreeMap::from([
            ("network".to_owned(), json!("deny")),
            ("external_resources".to_owned(), json!("deny")),
            ("scripts".to_owned(), json!("stripped")),
            ("mail_count".to_owned(), json!(mail_count)),
        ]),
        steps,
        changes: ChangeSet {
            preserved: vec![
                "every mail, in original order".to_owned(),
                "normalized textual content".to_owned(),
            ],
            changed: vec![format!(
                "the mailbox renders as one {} document",
                if target == "pdf" {
                    "PDF"
                } else {
                    target.as_str()
                }
            )],
            dropped: vec![
                "attachments and non-text MIME parts".to_owned(),
                "scripts, event handlers, and remote resources".to_owned(),
            ],
            unknown: vec!["visual fidelity of the original HTML".to_owned()],
        },
        validators,
        network_policy: NetworkPolicy::Deny,
        output_path: Some(output_path),
        estimated_output_bytes: None,
    };
    plan.plan_hash = deterministic_plan_hash(&plan)?;
    Ok(plan)
}

/// Executes the MBOX export. The pdf target stages per-mail HTML, runs each
/// through the html→pdf lane (their own plans + receipts), merges with qpdf,
/// and proves page conservation (sum of per-mail pages == merged pages).
///
/// # Errors
///
/// Parse/write/engine/validation errors; staging is cleaned on every path.
pub async fn execute_mbox_export(
    probe: &Probe,
    plan: &Plan,
    job_id: Uuid,
    cancellation: tokio_util::sync::CancellationToken,
) -> Result<(PathBuf, ValidationReport)> {
    let output = plan.output_path.clone().ok_or_else(|| {
        AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Execute,
            "MBOX export Plan has no output path",
            "Choose an output path.",
        )
    })?;
    if output.exists() {
        return Err(AnoleError::new(
            ErrorCode::OutputConflict,
            Stage::Execute,
            "The MBOX export destination already exists",
            "Choose another output path and retry.",
        ));
    }
    let mails = parse_mbox_file(&probe.artifact.canonical_path)?;
    let parent = output.parent().ok_or_else(|| {
        AnoleError::new(
            ErrorCode::InputInvalid,
            Stage::Plan,
            "Resolved output path has no parent directory",
            "Choose a complete output path.",
        )
    })?;
    let staging = parent.join(format!(".fw-mbox-{job_id}"));
    std::fs::create_dir(&staging).map_err(|error| {
        AnoleError::new(
            ErrorCode::StorageFailed,
            Stage::Execute,
            "Unable to create the MBOX staging directory",
            "Choose an output directory you can write to.",
        )
        .with_diagnostic(error.to_string())
    })?;
    let outcome = run_mbox_export(&mails, plan, job_id, cancellation, &output, &staging).await;
    if let Err(error) = std::fs::remove_dir_all(&staging)
        && outcome.is_ok()
    {
        return Err(AnoleError::new(
            ErrorCode::StorageFailed,
            Stage::Commit,
            "Unable to remove the MBOX staging directory",
            "Remove the leftover staging directory manually.",
        )
        .with_diagnostic(error.to_string()));
    }
    let (path, mut report) = outcome?;
    report.job_id = job_id;
    Ok((path, report))
}

async fn run_mbox_export(
    mails: &[MboxMail],
    plan: &Plan,
    job_id: Uuid,
    cancellation: tokio_util::sync::CancellationToken,
    output: &Path,
    staging: &Path,
) -> Result<(PathBuf, ValidationReport)> {
    let target = plan.target_format.as_str();
    if target == "pdf" {
        // Boxing breaks the async recursion execute_plan -> mbox -> execute_plan.
        Box::pin(execute_mbox_pdf(
            mails,
            plan,
            job_id,
            cancellation,
            output,
            staging,
        ))
        .await
    } else {
        let rendered = render_mbox(mails, target);
        tokio::task::spawn_blocking({
            let output = output.to_path_buf();
            let rendered = rendered.clone();
            move || std::fs::write(&output, rendered)
        })
        .await
        .map_err(|error| worker_error(&error))?
        .map_err(|error| write_error(&error))?;
        let output_probe = match crate::document::inspect_document(output).await {
            Ok(probe) => probe,
            Err(error) => {
                let _ = std::fs::remove_file(output);
                return Err(error);
            }
        };
        let rendered_text = if target == "html" {
            crate::document::html_text(&rendered)
                .ok()
                .unwrap_or_else(|| rendered.clone())
        } else {
            rendered.clone()
        };
        let report = build_mbox_report(
            mails,
            plan,
            job_id,
            match target {
                "html" => "html",
                "md" => "markdown",
                _ => "plain",
            },
            &output_probe.format.id,
            output,
            None,
            None,
            rendered_text.as_str(),
        );
        if report.status == ValidationStatus::Fail {
            let _ = std::fs::remove_file(output);
        }
        Ok((output.to_path_buf(), report))
    }
}

/// 拼接逐封渲染结果：每封前置分隔标记行，HTML 走同一净化管线，md 复用
/// EML 的 `render_md`。
fn render_mbox(mails: &[MboxMail], target: &str) -> String {
    let total = mails.len();
    mails
        .iter()
        .enumerate()
        .map(|(index, mail)| {
            let separator = mail_separator(index, total);
            if target == "html" {
                let body = eml::render_html(&mail.email);
                if let Some(start) = body.find("<body")
                    && let Some(open_end) = body[start..].find('>')
                {
                    let insert_at = start + open_end + 1;
                    return format!(
                        "{}<h3>{separator} {}</h3>{}",
                        &body[..insert_at],
                        mail.email.subject.as_deref().unwrap_or(""),
                        &body[insert_at..]
                    );
                }
                format!("<html><body><h3>{separator}</h3>{body}</body></html>")
            } else if target == "md" {
                format!("{}\n\n{}", separator, eml::render_md(&mail.email))
            } else {
                format!("{}\n{}", separator, eml::render_txt(&mail.email))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[allow(clippy::too_many_lines)]
async fn execute_mbox_pdf(
    mails: &[MboxMail],
    plan: &Plan,
    job_id: Uuid,
    cancellation: tokio_util::sync::CancellationToken,
    output: &Path,
    staging: &Path,
) -> Result<(PathBuf, ValidationReport)> {
    use crate::runner::execute_plan;
    use crate::workflow::prepare_conversion;

    let total = mails.len();
    let mut staged_pdfs = Vec::new();
    for (index, _mail) in mails.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Err(AnoleError::new(
                ErrorCode::Cancelled,
                Stage::Execute,
                "MBOX export was cancelled",
                "Retry when ready.",
            ));
        }
        let html_path = staging.join(format!("mail-{index}.html"));
        let packet = render_single_html_packet(&mails[index], index, total);
        tokio::task::spawn_blocking({
            let path = html_path.clone();
            let packet = packet.clone();
            move || std::fs::write(&path, packet)
        })
        .await
        .map_err(|error| worker_error(&error))?
        .map_err(|error| write_error(&error))?;
        let pdf_path = staging.join(format!("mail-{index}.pdf"));
        let request = PlanRequest {
            target_format: "pdf".to_owned(),
            output_path: Some(pdf_path.clone()),
            ..PlanRequest::default()
        };
        let (probe, segment_plan, validation_engine) =
            prepare_conversion(&html_path, &request).await?;
        let result = execute_plan(
            &probe,
            &segment_plan,
            &validation_engine,
            Uuid::new_v4(),
            cancellation.clone(),
        )
        .await?;
        staged_pdfs.push(result.output_path);
    }
    // 逐封页数求和（每页一条 stream）。
    let pdfinfo = crate::doctor::inspect_engine("pdfinfo").await?;
    let mut expected_pages = 0_usize;
    for pdf in &staged_pdfs {
        let probe = crate::pdf::inspect_pdf(pdf, &pdfinfo).await?;
        expected_pages += probe.streams.len();
    }
    let merged = staging.join("merged.pdf");
    let qpdf = crate::doctor::inspect_engine("qpdf").await?;
    run_qpdf_merge(&qpdf, &staged_pdfs, &merged).await?;
    let merged_probe = crate::pdf::inspect_pdf(&merged, &pdfinfo).await?;
    let observed_pages = merged_probe.streams.len();
    let pdftotext = crate::doctor::inspect_engine("pdftotext").await?;
    let extracted = extract_pdf_text(&merged, &pdftotext).await?;
    let report = build_mbox_report(
        mails,
        plan,
        job_id,
        "pdf",
        &merged_probe.format.id,
        &merged,
        Some(expected_pages),
        Some(observed_pages),
        &extracted,
    );
    if report.status == ValidationStatus::Fail {
        return Err(AnoleError::new(
            ErrorCode::ValidationFailed,
            Stage::Validate,
            "MBOX PDF failed required validation",
            "Inspect the validation report and choose another Plan.",
        )
        .with_diagnostic(serde_json::to_string(&report).unwrap_or_default()));
    }
    if output.exists() {
        return Err(AnoleError::new(
            ErrorCode::OutputConflict,
            Stage::Commit,
            "The MBOX destination appeared while conversion was running",
            "Choose another output path.",
        ));
    }
    crate::runner::commit_path_no_replace(&merged, output)?;
    Ok((output.to_path_buf(), report))
}

/// 单封 HTML 包：分隔标记 + 净化后的邮件 body 片段。只嵌片段不嵌整文档
/// ——嵌套 `<html>` 会让 pandox→DOCX 中间层的语义 token 守恒漂移。
fn render_single_html_packet(mail: &MboxMail, index: usize, total: usize) -> String {
    let separator = mail_separator(index, total);
    let subject = mail.email.subject.as_deref().unwrap_or("");
    let rendered = eml::render_html(&mail.email);
    let fragment = extract_html_body_fragment(&rendered);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body><h3>{separator} {subject}</h3>{fragment}</body></html>"
    )
}

/// 提取渲染结果 `<body>…</body>` 之间的片段；找不到标记时回退原文。
fn extract_html_body_fragment(rendered: &str) -> &str {
    let lowered_start = rendered.to_ascii_lowercase();
    let Some(start) = lowered_start.find("<body") else {
        return rendered;
    };
    let Some(open_end) = rendered[start..].find('>') else {
        return rendered;
    };
    let body_start = start + open_end + 1;
    let Some(end) = lowered_start.rfind("</body>") else {
        return rendered;
    };
    if end < body_start {
        return rendered;
    }
    &rendered[body_start..end]
}

async fn run_qpdf_merge(qpdf: &EngineIdentity, inputs: &[PathBuf], output: &Path) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let mut command = tokio::process::Command::new(&qpdf.binary_path);
    command.arg("--empty").arg("--pages");
    for input in inputs {
        command.arg(input).arg("1-z");
    }
    command.arg("--").arg(output);
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            AnoleError::new(
                ErrorCode::EngineIncompatible,
                Stage::Execute,
                "Unable to start qpdf",
                "Run doctor and verify the qpdf engine.",
            )
            .with_diagnostic(error.to_string())
        })?;
    let mut stderr = Vec::new();
    if let Some(mut stream) = child.stderr.take() {
        stream.read_to_end(&mut stderr).await.ok();
    }
    let status = child.wait().await.map_err(|error| {
        AnoleError::new(
            ErrorCode::ExecutionFailed,
            Stage::Execute,
            "Unable to wait for qpdf",
            "Retry the conversion.",
        )
        .with_diagnostic(error.to_string())
    })?;
    if !status.success() {
        return Err(AnoleError::new(
            ErrorCode::ExecutionFailed,
            Stage::Execute,
            "qpdf could not merge the per-mail PDFs",
            "Inspect the per-mail PDFs and qpdf availability.",
        )
        .with_diagnostic(String::from_utf8_lossy(&stderr).into_owned()));
    }
    Ok(())
}

async fn extract_pdf_text(path: &Path, pdftotext: &EngineIdentity) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new(&pdftotext.binary_path)
            .arg(path)
            .arg("-")
            .output(),
    )
    .await
    .map_err(|_| {
        AnoleError::new(
            ErrorCode::ExecutionFailed,
            Stage::Validate,
            "PDF text extraction timed out",
            "Retry the conversion.",
        )
        .retryable(true)
    })?
    .map_err(|error| {
        AnoleError::new(
            ErrorCode::EngineIncompatible,
            Stage::Validate,
            "Unable to start pdftotext",
            "Run doctor and verify the Poppler utilities.",
        )
        .with_diagnostic(error.to_string())
    })?;
    if !output.status.success() {
        return Err(AnoleError::new(
            ErrorCode::ValidationFailed,
            Stage::Validate,
            "pdftotext could not read the merged PDF",
            "Inspect the merged PDF.",
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn build_mbox_report(
    mails: &[MboxMail],
    plan: &Plan,
    job_id: Uuid,
    expected_format: &str,
    observed_format: &str,
    output: &Path,
    expected_pages: Option<usize>,
    observed_pages: Option<usize>,
    extracted_text: &str,
) -> ValidationReport {
    let total = mails.len();
    let separators_present = (0..total)
        .filter(|index| extracted_text.contains(&mail_separator(*index, total)))
        .count();
    let mut checks = vec![
        check(
            "MBOX_TARGET_FORMAT",
            observed_format == expected_format,
            json!(expected_format),
            json!(observed_format),
            "Detected output format.",
        ),
        check(
            "MBOX_MAIL_SEPARATORS",
            separators_present == total,
            json!(total),
            json!(separators_present),
            "Every mail separator survives into the output text layer.",
        ),
    ];
    if plan.target_format == "pdf" {
        let expected = expected_pages.unwrap_or_default();
        let observed = observed_pages.unwrap_or_default();
        checks.push(check(
            "MBOX_PAGE_CONSERVATION",
            expected == observed && observed > 0,
            json!(expected),
            json!(observed),
            "Merged page count equals the sum of per-mail pages.",
        ));
    }
    let report_status = checks.iter().fold(ValidationStatus::Pass, |state, item| {
        state.worst(item.status)
    });
    let output_artifact = identify_artifact_sync_summary(output);
    ValidationReport {
        schema_version: SCHEMA_VERSION,
        report_id: Uuid::new_v4(),
        job_id,
        plan_hash: plan.plan_hash.clone(),
        status: report_status,
        input: ArtifactSummary {
            display_path: None,
            format_id: "mbox".to_owned(),
            size_bytes: 0,
            fast_fingerprint: String::new(),
            full_blake3: None,
        },
        output: output_artifact,
        engines: plan.steps.iter().map(|step| step.engine.clone()).collect(),
        checks,
        intentional_changes: plan.changes.changed.clone(),
        redaction: ReportRedaction {
            paths_redacted: false,
            metadata_values_redacted: true,
        },
    }
}

fn identify_artifact_sync_summary(path: &Path) -> ArtifactSummary {
    let metadata = std::fs::metadata(path).ok();
    ArtifactSummary {
        display_path: Some(path.to_string_lossy().into_owned()),
        format_id: "pdf".to_owned(),
        size_bytes: metadata.map(|value| value.len()).unwrap_or_default(),
        fast_fingerprint: String::new(),
        full_blake3: None,
    }
}

fn check(
    code: &str,
    passed: bool,
    expected: Value,
    observed: Value,
    message: &str,
) -> ValidationCheck {
    ValidationCheck {
        code: code.to_owned(),
        status: if passed {
            ValidationStatus::Pass
        } else {
            ValidationStatus::Fail
        },
        required: true,
        expected,
        observed,
        evidence: "Anole native MBOX adapter".to_owned(),
        message: message.to_owned(),
    }
}

fn worker_error(error: &tokio::task::JoinError) -> AnoleError {
    AnoleError::new(
        ErrorCode::Internal,
        Stage::Execute,
        "MBOX export worker task failed",
        "Retry the conversion.",
    )
    .with_diagnostic(error.to_string())
}

fn write_error(error: &std::io::Error) -> AnoleError {
    AnoleError::new(
        ErrorCode::ExecutionFailed,
        Stage::Execute,
        "Unable to write the MBOX export output",
        "Check the destination directory and retry.",
    )
    .with_diagnostic(error.to_string())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{MBOX_ENGINE_ID, MboxVariant, split_mbox_bytes, split_mbox_bytes_variant};

    const THREE_MAILS: &str = "From alice@example.org Fri Sep  4 10:00:00 2026\r
From: Alice <alice@example.org>\r
To: bob@example.org\r
Subject: First mail 440010147700\r
\r
ELECTRIC body one 998877.\r
>From the escaped line stays.\r
\r
From carol@example.org Fri Sep  4 11:00:00 2026\r
From: Carol <carol@example.org>\r
To: bob@example.org\r
Subject: =?UTF-8?B?5LiW55WM6YKu5Lu=?= MAILTOKEN2\r
\r
Body two with 中文内容 MAIL2TOKEN.\r
\r
From dave@example.org Fri Sep  4 12:00:00 2026\r
From: Dave <dave@example.org>\r
Subject: Third plain mail\r
Content-Type: text/html\r
\r
<html><body><p>MAIL3TOKEN</p><script>alert(1)</script></body></html>\r
";

    fn builtin_engine() -> anole_engine_sdk::EngineIdentity {
        anole_engine_sdk::EngineIdentity {
            engine_id: MBOX_ENGINE_ID.to_owned(),
            version: "0.1.0".to_owned(),
            binary_path: std::path::PathBuf::from("anole.exe"),
            binary_sha256: "0".repeat(64),
            manifest_sha256: None,
            build_configuration: None,
            certification: anole_engine_sdk::Certification::Experimental,
        }
    }

    fn qpdf_engine() -> anole_engine_sdk::EngineIdentity {
        anole_engine_sdk::EngineIdentity {
            engine_id: "qpdf".to_owned(),
            ..builtin_engine()
        }
    }

    #[test]
    fn splits_three_mails_and_keeps_quoted_from_lines_by_default() {
        let (variant, messages) =
            split_mbox_bytes_variant(THREE_MAILS.as_bytes(), None).expect("split");
        assert_eq!(messages.len(), 3);
        // 无 Content-Length 时无法区分 mboxrd/mboxo：保守假设 mboxo，
        // `>From ` 行按正文原样保留（漏转义比错误 unescape 安全）。
        assert_eq!(variant, MboxVariant::O);
        let first = String::from_utf8(messages[0].clone()).expect("utf8");
        assert!(first.contains(">From the escaped line stays."));
    }

    #[test]
    fn explicit_mboxrd_variant_unescapes_one_level() {
        let (_, messages) =
            split_mbox_bytes_variant(THREE_MAILS.as_bytes(), Some(MboxVariant::Rd)).expect("split");
        let first = String::from_utf8(messages[0].clone()).expect("utf8");
        assert!(first.contains("From the escaped line stays."));
        assert!(!first.contains(">From the escaped line stays."));
    }

    #[test]
    fn mboxcl_content_length_keeps_from_lines_in_body() {
        // 正文里含「空行 + From_ 行」（启发式会误切），由 Content-Length
        // 精确保护；第二封无该头，回退 From_ 启发式。
        let body_one = "CL body one MAILCL1TOKEN\r
\r
From forged@example.org Thu Sep  3 10:00:00 2026\r
CL body continues 中文 MAILCL1B\r
";
        let mailbox = format!(
            "From alice@example.org Thu Sep  3 09:00:00 2026\r
From: Alice <alice@example.org>\r
Subject: CL one 440010147700\r
Content-Length: {}\r
\r
{}From bob@example.org Fri Sep  4 11:00:00 2026\r
From: Bob <bob@example.org>\r
Subject: =?UTF-8?B?56ys5LqM5bCB?= MAILCL2\r
\r
CL body two MAILCL2TOKEN.\r
",
            body_one.len(),
            body_one
        );
        let (variant, messages) =
            split_mbox_bytes_variant(mailbox.as_bytes(), None).expect("split");
        assert_eq!(variant, MboxVariant::Cl);
        assert_eq!(
            messages.len(),
            2,
            "Content-Length must override the From_ heuristic"
        );
        let first = String::from_utf8(messages[0].clone()).expect("utf8");
        assert!(first.contains("From forged@example.org"));
        assert!(first.contains("CL body continues"));
        let second = crate::eml::parse_eml_bytes(&messages[1]).expect("parse");
        assert_eq!(second.subject.as_deref(), Some("第二封 MAILCL2"));
    }

    #[test]
    fn mboxcl_content_length_mismatch_fails_closed() {
        let body = "0123456789abcdefghijklmnopqrstuvwxyz\r\nstill more body\r\n";
        // 声明偏短：正文残留不是 From_ 行 → 拒绝，不静默截断。
        let short = format!(
            "From alice@example.org Thu Sep  3 09:00:00 2026\r
Content-Length: 30\r
\r
{body}"
        );
        let error = split_mbox_bytes(short.as_bytes()).expect_err("short Content-Length");
        assert_eq!(error.code, crate::ErrorCode::InputInvalid);
        // 声明超出文件：同样拒绝。
        let over = format!(
            "From alice@example.org Thu Sep  3 09:00:00 2026\r
Content-Length: {}\r
\r
{body}",
            body.len() + 512
        );
        let error = split_mbox_bytes(over.as_bytes()).expect_err("overlong Content-Length");
        assert_eq!(error.code, crate::ErrorCode::InputInvalid);
    }

    #[test]
    fn mboxo_quoted_from_lines_stay_verbatim() {
        let mailbox = "From alice@example.org Thu Sep  3 09:00:00 2026\r
From: Alice <alice@example.org>\r
Subject: =?UTF-8?B?5rWL6K+V6YKu5Lu2?= MBOXOTOKEN\r
\r
Hello 中文正文 MBOXO1.\r
>From the quoted line stays verbatim.\r
";
        let (variant, messages) =
            split_mbox_bytes_variant(mailbox.as_bytes(), None).expect("split");
        assert_eq!(variant, MboxVariant::O);
        let first = String::from_utf8(messages[0].clone()).expect("utf8");
        assert!(first.contains(">From the quoted line stays verbatim."));
        let parsed = crate::eml::parse_eml_bytes(&messages[0]).expect("parse");
        assert_eq!(parsed.subject.as_deref(), Some("测试邮件 MBOXOTOKEN"));
    }

    #[test]
    fn empty_or_garbage_mbox_fails_closed() {
        assert!(split_mbox_bytes(b"").is_err());
        assert!(split_mbox_bytes(b"no from lines at all\njust text\n").is_err());
    }

    #[test]
    fn single_mail_mbox_round_trips() {
        let single = "From a@example.org Thu Sep  3 09:00:00 2026\r
From: a@example.org\r
Subject: Solo 440\r
\r
solo body\r
";
        let messages = split_mbox_bytes(single.as_bytes()).expect("split");
        assert_eq!(messages.len(), 1);
        let parsed = crate::eml::parse_eml_bytes(&messages[0]).expect("parse");
        assert_eq!(parsed.subject.as_deref(), Some("Solo 440"));
    }

    #[tokio::test]
    async fn mbox_txt_and_html_export_validate() {
        let directory = TempDir::new().expect("tempdir");
        let source = directory.path().join("sample.mbox");
        std::fs::write(&source, THREE_MAILS).expect("write mbox");
        let probe = super::inspect_mbox(&source).await.expect("probe");
        assert_eq!(
            probe.streams[0].properties.get("mail_count"),
            Some(&serde_json::json!(3))
        );
        let engine = builtin_engine();
        for target in ["txt", "html"] {
            let output = directory.path().join(format!("out.{target}"));
            let plan =
                super::plan_mbox_export(&probe, output.clone(), &engine, &qpdf_engine(), target)
                    .expect("plan");
            let (path, report) = super::execute_mbox_export(
                &probe,
                &plan,
                uuid::Uuid::new_v4(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("execute");
            assert!(path.is_file());
            assert_ne!(report.status, crate::domain::ValidationStatus::Fail);
            let text = std::fs::read_to_string(&path).expect("read");
            assert!(text.contains("==== Anole Mail 1/3 ===="));
            assert!(text.contains("==== Anole Mail 3/3 ===="));
            if target == "html" {
                assert!(!text.contains("<script>"), "scripts stripped");
            }
        }
    }

    #[tokio::test]
    async fn mboxcl_txt_export_keeps_every_mail_separator() {
        let directory = TempDir::new().expect("tempdir");
        let body_one = "CL body one MAILCL1TOKEN\r\n\r\nFrom forged@example.org Thu Sep  3 10:00:00 2026\r\nCL body continues\r\n";
        let mailbox = format!(
            "From alice@example.org Thu Sep  3 09:00:00 2026\r
From: Alice <alice@example.org>\r
Subject: CL one 440010147700\r
Content-Length: {}\r
\r
{}From bob@example.org Fri Sep  4 11:00:00 2026\r
From: Bob <bob@example.org>\r
Subject: =?UTF-8?B?56ys5LqM5bCB?= MAILCL2\r
\r
CL body two MAILCL2TOKEN.\r
",
            body_one.len(),
            body_one
        );
        let source = directory.path().join("cl.mbox");
        std::fs::write(&source, mailbox).expect("write mbox");
        let probe = super::inspect_mbox(&source).await.expect("probe");
        assert_eq!(probe.format.container.as_deref(), Some("mboxcl"));
        assert_eq!(
            probe.streams[0].properties.get("mail_count"),
            Some(&serde_json::json!(2))
        );
        assert_eq!(
            probe.streams[0].properties.get("mbox_variant"),
            Some(&serde_json::json!("mboxcl"))
        );
        assert_eq!(
            probe.streams[0].properties.get("variant_basis"),
            Some(&serde_json::json!("content-length-header"))
        );
        let plan = super::plan_mbox_export(
            &probe,
            directory.path().join("out.txt"),
            &builtin_engine(),
            &qpdf_engine(),
            "txt",
        )
        .expect("plan");
        let (path, report) = super::execute_mbox_export(
            &probe,
            &plan,
            uuid::Uuid::new_v4(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("execute");
        assert!(path.is_file());
        assert_ne!(report.status, crate::domain::ValidationStatus::Fail);
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("==== Anole Mail 1/2 ===="));
        assert!(text.contains("==== Anole Mail 2/2 ===="));
        assert!(text.contains("From forged@example.org"));
    }

    #[tokio::test]
    async fn remote_resource_mbox_is_policy_blocked() {
        let directory = TempDir::new().expect("tempdir");
        let source = directory.path().join("remote.mbox");
        std::fs::write(
            &source,
            "From a@example.org Thu Sep  3 09:00:00 2026\r
From: a@example.org\r
Subject: tracker\r
Content-Type: text/html\r
\r
<html><body><img src=\"https://tracker.example.org/p.gif\">x</body></html>\r
",
        )
        .expect("write");
        let probe = super::inspect_mbox(&source).await.expect("probe");
        assert_eq!(
            probe.streams[0].properties.get("has_external_resource"),
            Some(&serde_json::json!(true))
        );
        let plan = super::plan_mbox_export(
            &probe,
            directory.path().join("blocked.html"),
            &builtin_engine(),
            &qpdf_engine(),
            "html",
        );
        assert_eq!(
            plan.expect_err("remote resource must block").code,
            crate::ErrorCode::PolicyBlocked
        );
    }

    #[tokio::test]
    async fn mbox_pdf_export_validates_when_engines_exist() {
        let directory = TempDir::new().expect("tempdir");
        let source = directory.path().join("sample.mbox");
        std::fs::write(&source, THREE_MAILS).expect("write mbox");
        let probe = super::inspect_mbox(&source).await.expect("probe");
        // The composite needs the html→pdf lane plus qpdf/poppler; skip when
        // this environment has none (e.g. plain CI runners).
        for engine in ["qpdf", "pdfinfo", "pdftotext"] {
            if crate::doctor::inspect_engine(engine).await.is_err() {
                eprintln!("skipping mbox pdf e2e: engine {engine} missing");
                return;
            }
        }
        let qpdf = crate::doctor::inspect_engine("qpdf").await.expect("qpdf");
        let output = directory.path().join("out.pdf");
        let plan = super::plan_mbox_export(&probe, output.clone(), &builtin_engine(), &qpdf, "pdf")
            .expect("plan");
        let (path, report) = super::execute_mbox_export(
            &probe,
            &plan,
            uuid::Uuid::new_v4(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("execute");
        assert!(path.is_file());
        assert_eq!(report.status, crate::domain::ValidationStatus::Pass);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.code == "MBOX_PAGE_CONSERVATION"
                    && check.status == crate::domain::ValidationStatus::Pass)
        );
    }
}
