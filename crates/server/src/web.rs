//! Web-service extensions for the local API server (web track W1).
//!
//! Adds browser-oriented endpoints on top of the existing surface:
//! multipart uploads with an extension whitelist and a size cap, a queued
//! job registry with per-IP admission and a global conversion-slot limit,
//! TTL-scoped temporary files (inputs are deleted as soon as their
//! conversion reaches a terminal state; outputs live until their TTL), and
//! optional same-origin SPA static hosting (`ANOLE_WEB_DIR`).
//!
//! Every conversion still flows through the shared core services, so each
//! successful job carries the full `ValidationReport` acceptance evidence.

use std::collections::HashMap;
use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anole_core::error::AnoleError;
use anole_core::{
    ConversionService, ErrorCode, PlanRequest, ReportService, capability_snapshot_for_input,
    prepare_conversion,
};
use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};
use tokio::io::AsyncWriteExt as _;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::routes::{ApiError, AppState, ValidJson, default_reports_directory, open_job_store};

/// Default upload cap (spec: 50 MiB, Render-free disk constraint).
pub const DEFAULT_MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;
/// Default hard-delete TTL for uploaded inputs and finished outputs.
pub const DEFAULT_TTL_SECS: u64 = 3600;
/// Default wall-clock budget for a single conversion (spec: 120s).
pub const DEFAULT_CONVERSION_TIMEOUT_SECS: u64 = 120;
/// Default simultaneous conversions (Lite deployment = 1, spec architecture).
pub const DEFAULT_MAX_CONCURRENT_CONVERSIONS: usize = 1;

const SWEEP_INTERVAL_SECS: u64 = 60;

/// Input extensions the web surface accepts, aligned with the support
/// matrix (structured, documents, office, media, mail, raster/RAW, archives).
const INPUT_EXTENSIONS: &[&str] = &[
    // structured
    "json", "csv", "yaml", "yml", "xml", "txt", // markup documents
    "md", "markdown", "html", "htm", // pdf
    "pdf", // office
    "docx", "xlsx", "pptx", "odt", "xls", "xlsm", "xlsb", // media
    "mp4", "webm", "mkv", "mov", "avi", "m4v", "flv", "wmv", "mpg", "mpeg", "gif", "wav", "flac",
    "aac", "m4a", "ogg", "opus", "mp3", // raster and camera RAW
    "png", "jpg", "jpeg", "webp", "avif", "tiff", "tif", "bmp", "heic", "heif", "psd", "dng",
    "cr2", "cr3", "arw", "nef", "orf", "rw2", "pef", "raf", // mail
    "eml", "msg", "mbox", // archives
    "zip", "7z", "tar", "gz", "tgz", "taz",
];

/// Raster inputs that can reach a single-file bitmap output; every other
/// input to jpg/png goes through a paged directory render (mirrors the
/// desktop `isDirectoryOutput` model).
const SINGLE_FILE_BITMAP_INPUTS: &[&str] = &[
    "heic", "heif", "psd", "dng", "cr2", "cr3", "arw", "nef", "orf", "rw2", "pef", "raf", "tiff",
    "tif", "bmp",
];

/// Runtime knobs for the web track; every field has an environment override.
#[derive(Clone, Debug)]
pub struct WebConfig {
    /// Root directory for `uploads/` and `jobs/`.
    pub root_dir: PathBuf,
    pub max_upload_bytes: usize,
    pub ttl_secs: u64,
    pub conversion_timeout_secs: u64,
    pub max_concurrent_conversions: usize,
    /// Optional SPA static directory (`ANOLE_WEB_DIR`); when unset the
    /// router serves no static fallback.
    pub static_dir: Option<PathBuf>,
}

impl WebConfig {
    /// Builds the config from the state-database directory plus environment
    /// overrides; parse failures fall back to the documented defaults.
    #[must_use]
    pub fn from_env(state_root: &Path) -> Self {
        let state_root = if state_root.as_os_str().is_empty() {
            Path::new(".")
        } else {
            state_root
        };
        Self {
            root_dir: state_root.join("web"),
            max_upload_bytes: env_parse("ANOLE_WEB_MAX_UPLOAD_BYTES", DEFAULT_MAX_UPLOAD_BYTES),
            ttl_secs: env_parse("ANOLE_WEB_INPUT_TTL_SECS", DEFAULT_TTL_SECS),
            conversion_timeout_secs: env_parse(
                "ANOLE_WEB_CONVERSION_TIMEOUT_SECS",
                DEFAULT_CONVERSION_TIMEOUT_SECS,
            ),
            max_concurrent_conversions: env_parse(
                "ANOLE_WEB_MAX_CONCURRENT_CONVERSIONS",
                DEFAULT_MAX_CONCURRENT_CONVERSIONS,
            ),
            static_dir: std::env::var_os("ANOLE_WEB_DIR").map(PathBuf::from),
        }
    }

    fn uploads_dir(&self) -> PathBuf {
        self.root_dir.join("uploads")
    }

    fn jobs_dir(&self) -> PathBuf {
        self.root_dir.join("jobs")
    }
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    if let Ok(raw) = std::env::var(key)
        && !raw.trim().is_empty()
    {
        if let Ok(parsed) = raw.trim().parse() {
            parsed
        } else {
            eprintln!("anole-server: ignoring invalid {key}={raw}; using the default");
            default
        }
    } else {
        default
    }
}

/// A live uploaded input awaiting conversion.
#[derive(Clone, Debug)]
pub struct UploadRecord {
    pub upload_id: String,
    pub file_name: String,
    pub stored_path: PathBuf,
    pub size_bytes: u64,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
}

/// Terminal-capable web job tracked in memory (`W2` moves this to `SQLite`).
#[derive(Clone, Debug)]
pub struct WebJob {
    pub job_id: String,
    pub upload_id: String,
    pub source_file_name: String,
    pub client_ip: Option<IpAddr>,
    pub target_format: String,
    pub request_fields: Map<String, Value>,
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub state: WebJobState,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
    pub download_name: String,
    pub archive_path: Option<PathBuf>,
    pub validation: Option<Value>,
    pub error: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebJobState {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl WebJobState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

impl WebJob {
    fn is_active(&self) -> bool {
        matches!(self.state, WebJobState::Queued | WebJobState::Running)
    }
}

/// Shared web state: upload registry, job registry, and the global
/// conversion-slot semaphore.
#[derive(Clone, Debug)]
pub struct WebState {
    inner: Arc<WebInner>,
}

#[derive(Debug)]
struct WebInner {
    config: WebConfig,
    uploads: Mutex<HashMap<String, UploadRecord>>,
    jobs: Mutex<HashMap<String, WebJob>>,
    conversion_slots: Semaphore,
}

impl WebState {
    #[must_use]
    pub fn new(config: WebConfig) -> Self {
        let slots = config.max_concurrent_conversions.max(1);
        Self {
            inner: Arc::new(WebInner {
                config,
                uploads: Mutex::new(HashMap::new()),
                jobs: Mutex::new(HashMap::new()),
                conversion_slots: Semaphore::new(slots),
            }),
        }
    }

    #[must_use]
    pub fn config(&self) -> &WebConfig {
        &self.inner.config
    }

    /// Removes expired uploads and finished jobs plus their files. Public so
    /// tests and operators can trigger a sweep; `spawn_ttl_sweeper` drives it.
    pub async fn sweep_expired(&self) {
        let now = SystemTime::now();
        let mut uploads = self.inner.uploads.lock().await;
        uploads.retain(|_, record| {
            let alive = record.expires_at > now;
            if !alive {
                let _unused = std::fs::remove_file(&record.stored_path);
            }
            alive
        });
        drop(uploads);

        let mut jobs = self.inner.jobs.lock().await;
        jobs.retain(|_, job| {
            let alive = job.is_active() || job.expires_at > now;
            if !alive {
                remove_output_artifacts(job);
            }
            alive
        });
    }
}

/// Background TTL sweeper; `main` spawns one per server process.
pub fn spawn_ttl_sweeper(web: WebState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(SWEEP_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            web.sweep_expired().await;
        }
    })
}

fn remove_output_artifacts(job: &WebJob) {
    if job.output_path.is_dir() {
        let _unused = std::fs::remove_dir_all(&job.output_path);
    } else if job.output_path.is_file() {
        let _unused = std::fs::remove_file(&job.output_path);
    }
    if let Some(archive) = &job.archive_path
        && archive.is_file()
    {
        let _unused = std::fs::remove_file(archive);
    }
}

fn unix_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn now_plus(ttl_secs: u64) -> SystemTime {
    SystemTime::now()
        .checked_add(Duration::from_secs(ttl_secs))
        .unwrap_or_else(|| SystemTime::now() + Duration::from_secs(ttl_secs))
}

fn forwarded_client_ip(headers: &axum::http::HeaderMap) -> IpAddr {
    for name in ["x-forwarded-for", "x-real-ip"] {
        if let Some(value) = headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            && let Ok(ip) = value.parse::<IpAddr>()
        {
            return ip;
        }
    }
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

fn not_found(message: impl Into<String>, action: impl Into<String>) -> ApiError {
    let error: ApiError = AnoleError::new(
        ErrorCode::InputInvalid,
        anole_core::Stage::Inspect,
        message,
        action,
    )
    .into();
    error.with_status(StatusCode::NOT_FOUND)
}

fn invalid_input(message: impl Into<String>, action: impl Into<String>) -> ApiError {
    AnoleError::new(
        ErrorCode::InputInvalid,
        anole_core::Stage::Inspect,
        message,
        action,
    )
    .into()
}

fn storage_failure(message: impl Into<String>, diagnostic: Option<String>) -> ApiError {
    let error = AnoleError::new(
        ErrorCode::StorageFailed,
        anole_core::Stage::Store,
        message,
        "Retry the upload; if it keeps failing, check server disk space.",
    );
    let error = match diagnostic {
        Some(text) => error.with_diagnostic(text),
        None => error,
    };
    error.into()
}

fn resource_exhausted(message: impl Into<String>, action: impl Into<String>) -> ApiError {
    AnoleError::new(
        ErrorCode::ResourceExhausted,
        anole_core::Stage::Execute,
        message,
        action,
    )
    .into()
}

/// Sanitizes a client-supplied filename to a single safe path component.
fn sanitize_file_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "upload".to_owned()
    } else {
        trimmed.chars().take(120).collect()
    }
}

fn file_extension(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
}

fn normalize_target(target: &str) -> String {
    let normalized = target.trim().trim_start_matches('.').to_ascii_lowercase();
    match normalized.as_str() {
        "jpeg" => "jpg".to_owned(),
        "yml" => "yaml".to_owned(),
        other => other.to_owned(),
    }
}

/// `POST /v1/uploads` — multipart field `file`; returns the upload ticket.
///
/// # Errors
///
/// Returns a structured API error on invalid input, size-limit, or storage failures.
#[allow(clippy::too_many_lines)]
pub async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let config = state.web().config().clone();
    let uploads_dir = config.uploads_dir();
    std::fs::create_dir_all(&uploads_dir).map_err(|error| {
        storage_failure(
            "cannot create the uploads directory",
            Some(error.to_string()),
        )
    })?;

    let mut uploaded: Option<UploadRecord> = None;
    while let Some(mut field) = multipart.next_field().await.map_err(|error| {
        invalid_input(
            format!("invalid multipart body: {error}"),
            "Send the file as the `file` part of a multipart/form-data request.",
        )
    })? {
        if field.name() != Some("file") {
            continue;
        }
        let raw_name = field.file_name().map(str::to_owned);
        let Some(raw_name) = raw_name else {
            return Err(invalid_input(
                "the `file` part is missing its filename",
                "Attach the file with a filename so the format can be checked.",
            ));
        };
        let file_name = sanitize_file_name(&raw_name);
        let extension = file_extension(&file_name).unwrap_or_default();
        if !INPUT_EXTENSIONS.contains(&extension.as_str()) {
            return Err(invalid_input(
                format!("unsupported input extension: .{extension}"),
                "Check the supported input formats list and retry with a supported file.",
            ));
        }

        let upload_id = Uuid::new_v4().simple().to_string();
        let stored_path = uploads_dir.join(format!("{upload_id}.{extension}"));
        let mut writer = tokio::fs::File::create(&stored_path)
            .await
            .map_err(|error| {
                storage_failure(
                    format!("cannot store the upload: {}", stored_path.display()),
                    Some(error.to_string()),
                )
            })?;
        let mut size_bytes: u64 = 0;
        let mut too_large = false;
        while let Some(chunk) = field.chunk().await.map_err(|error| {
            invalid_input(
                format!("upload stream failed: {error}"),
                "Retry the upload.",
            )
        })? {
            size_bytes += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            if size_bytes > u64::try_from(config.max_upload_bytes).unwrap_or(u64::MAX) {
                too_large = true;
                break;
            }
            writer.write_all(&chunk).await.map_err(|error| {
                storage_failure("cannot write the upload", Some(error.to_string()))
            })?;
        }
        drop(writer);
        if too_large {
            let _unused = std::fs::remove_file(&stored_path);
            return Err(invalid_input(
                format!(
                    "upload exceeds the {} MiB limit",
                    config.max_upload_bytes / (1024 * 1024)
                ),
                "Compress or split the file, then retry.",
            ));
        }

        uploaded = Some(UploadRecord {
            upload_id: upload_id.clone(),
            file_name,
            stored_path,
            size_bytes,
            created_at: SystemTime::now(),
            expires_at: now_plus(config.ttl_secs),
        });
        // Only the first `file` part is accepted.
        break;
    }

    let record = uploaded.ok_or_else(|| {
        invalid_input(
            "multipart body has no `file` part",
            "Attach exactly one file part named `file`.",
        )
    })?;
    let response = json!({
        "upload_id": record.upload_id.clone(),
        "file_name": record.file_name.clone(),
        "size_bytes": record.size_bytes,
        "expires_at": unix_secs(record.expires_at),
        "ttl_secs": config.ttl_secs,
        "max_upload_bytes": config.max_upload_bytes,
    });
    state
        .web()
        .inner
        .uploads
        .lock()
        .await
        .insert(record.upload_id.clone(), record);
    Ok(Json(response))
}

async fn lookup_upload(state: &AppState, upload_id: &str) -> Result<UploadRecord, ApiError> {
    state
        .web()
        .inner
        .uploads
        .lock()
        .await
        .get(upload_id)
        .cloned()
        .ok_or_else(|| {
            not_found(
                format!("unknown or expired upload: {upload_id}"),
                "Upload the file again; unused uploads expire after one hour.",
            )
        })
}

/// `GET /v1/uploads/{upload_id}/capabilities` — target routes for the input.
///
/// # Errors
///
/// Returns a structured 404-style error for unknown upload ids.
pub async fn upload_capabilities(
    State(state): State<AppState>,
    axum::extract::Path(upload_id): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    let record = lookup_upload(&state, &upload_id).await?;
    if !record.stored_path.is_file() {
        return Err(not_found(
            "the uploaded input is no longer available",
            "Upload the file again.",
        ));
    }
    let snapshot = capability_snapshot_for_input(
        &record.stored_path,
        anole_core::EngineDiscoveryPolicy::for_current_build(),
    )
    .await;
    Ok(Json(
        serde_json::to_value(snapshot).unwrap_or_else(|_| json!({})),
    ))
}

/// `POST /v1/uploads/{upload_id}/plan` — preview the Plan without running it.
///
/// # Errors
///
/// Returns a structured API error when the upload is missing or the input cannot be planned.
pub async fn upload_plan(
    State(state): State<AppState>,
    axum::extract::Path(upload_id): axum::extract::Path<String>,
    ValidJson(body): ValidJson<Value>,
) -> Result<Json<Value>, ApiError> {
    let record = lookup_upload(&state, &upload_id).await?;
    let fields = request_fields(&body)?;
    let target = normalize_target(
        fields
            .get("target_format")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if target.is_empty() {
        return Err(invalid_input(
            "target_format is required",
            "Choose a target format.",
        ));
    }
    let output = planned_output_path(
        &state.web().config().jobs_dir().join("preview"),
        &record,
        &target,
    );
    let request = plan_request_from_value(&fields, &target, &output)?;
    let (probe, plan, _engine) = prepare_conversion(&record.stored_path, &request)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(json!({
        "probe": probe,
        "plan": plan,
        "plan_hash": plan.plan_hash,
    })))
}

/// `POST /v1/jobs` — enqueue a conversion for a live upload.
///
/// # Errors
///
/// Returns a structured API error on missing uploads, invalid targets, or admission limits.
pub async fn create_job(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    ValidJson(body): ValidJson<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let config = state.web().config().clone();
    let fields = request_fields(&body)?;
    let upload_id = fields
        .get("upload_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if upload_id.is_empty() {
        return Err(invalid_input(
            "upload_id is required",
            "Upload a file first, then submit the conversion.",
        ));
    }
    let record = lookup_upload(&state, &upload_id).await?;
    if !record.stored_path.is_file() {
        return Err(not_found(
            "the uploaded input is no longer available",
            "Upload the file again.",
        ));
    }
    let target = normalize_target(
        fields
            .get("target_format")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if target.is_empty() {
        return Err(invalid_input(
            "target_format is required",
            "Choose a target format.",
        ));
    }
    // 代理部署（Render/HF）下 ConnectInfo 是代理地址，真实客户端 IP
    // 以转发头为准；直连本地部署（测试/本机）没有这些头，按 localhost。
    let client_ip = forwarded_client_ip(&headers);

    let jobs_dir = config.jobs_dir();
    let job_id = Uuid::new_v4().hyphenated().to_string();
    let job_root = jobs_dir.join(&job_id);
    std::fs::create_dir_all(&job_root).map_err(|error| {
        storage_failure("cannot create the job directory", Some(error.to_string()))
    })?;

    let mut jobs = state.web().inner.jobs.lock().await;
    let active_for_ip = jobs
        .values()
        .filter(|job| job.is_active() && job.client_ip == Some(client_ip))
        .count();
    if active_for_ip > 0 {
        return Err(resource_exhausted(
            "this client already has a conversion in the queue",
            "Wait for the current conversion to finish, then submit again.",
        ));
    }

    let output_path = planned_output_path(&job_root, &record, &target);
    let job = WebJob {
        job_id: job_id.clone(),
        upload_id: record.upload_id.clone(),
        source_file_name: record.file_name.clone(),
        client_ip: Some(client_ip),
        target_format: target,
        request_fields: fields,
        input_path: record.stored_path,
        output_path,
        state: WebJobState::Queued,
        created_at: SystemTime::now(),
        expires_at: now_plus(config.ttl_secs),
        download_name: String::new(),
        archive_path: None,
        validation: None,
        error: None,
    };
    jobs.insert(job_id.clone(), job);
    drop(jobs);

    let state_db = state.state_db().to_path_buf();
    let web = state.web().clone();
    let spawned_job_id = job_id.clone();
    tokio::spawn(async move {
        execute_web_job(state_db, web, spawned_job_id).await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "job_id": job_id,
            "state": "queued",
            "poll_url": format!("/v1/jobs/{job_id}"),
        })),
    ))
}

fn request_fields(body: &Value) -> Result<Map<String, Value>, ApiError> {
    // Path-shaped fields are server-owned; strip them so a client cannot
    // redirect conversions at arbitrary server paths.
    const RESERVED: &[&str] = &[
        "input_path",
        "inputPath",
        "output_path",
        "outputPath",
        "inputs",
    ];
    let Value::Object(map) = body else {
        return Err(invalid_input(
            "request body must be a JSON object",
            "Send a JSON object with upload_id and target_format.",
        ));
    };
    Ok(map
        .iter()
        .filter(|(key, _)| !RESERVED.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect())
}

fn plan_request_from_value(
    fields: &Map<String, Value>,
    target: &str,
    output: &Path,
) -> Result<PlanRequest, ApiError> {
    let mut merged = serde_json::to_value(PlanRequest::default()).map_err(|error| {
        AnoleError::new(
            ErrorCode::Internal,
            anole_core::Stage::Plan,
            format!("cannot serialize default plan request: {error}"),
            "This is a server bug; please report it with the request body.",
        )
    })?;
    if let Value::Object(base) = &mut merged {
        base.extend(fields.clone());
        base.insert("target_format".to_owned(), json!(target));
        base.insert("output_path".to_owned(), json!(output));
    }
    serde_json::from_value(merged).map_err(|error| {
        invalid_input(
            format!("invalid PlanRequest fields: {error}"),
            "Fix the request body fields and retry.",
        )
    })
}

/// Mirrors the desktop `isDirectoryOutput` rule to pick the planned output
/// path shape before the core planner runs.
fn planned_output_path(job_root: &Path, record: &UploadRecord, target: &str) -> PathBuf {
    let stem = Path::new(&record.file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("upload");
    let directory_output = ["jpg", "png"].contains(&target)
        && !SINGLE_FILE_BITMAP_INPUTS.contains(
            &file_extension(&record.file_name)
                .unwrap_or_default()
                .as_str(),
        );
    if directory_output {
        job_root.join(format!("{stem}.converted-{target}-pages"))
    } else {
        job_root.join(format!("{stem}.converted.{target}"))
    }
}

fn download_name_for(source_file_name: &str, target: &str, archive: bool) -> String {
    let stem = Path::new(source_file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("upload");
    if archive {
        format!("{stem}.converted-{target}-pages.zip")
    } else {
        format!("{stem}.converted.{target}")
    }
}

async fn execute_web_job(state_db: PathBuf, web: WebState, job_id: String) {
    // Queue wait: hold one global conversion slot for the whole run.
    let permit = web
        .inner
        .conversion_slots
        .acquire()
        .await
        .expect("conversion semaphore is never closed");
    {
        let mut jobs = web.inner.jobs.lock().await;
        if let Some(job) = jobs.get_mut(&job_id)
            && job.state == WebJobState::Queued
        {
            job.state = WebJobState::Running;
        }
    }

    let job_snapshot = {
        let jobs = web.inner.jobs.lock().await;
        jobs.get(&job_id).cloned()
    };
    let Some(job) = job_snapshot else {
        return;
    };

    let timeout = Duration::from_secs(web.config().conversion_timeout_secs);
    let token = CancellationToken::new();
    let watchdog = {
        let token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            token.cancel();
        })
    };
    let outcome = tokio::select! {
        result = run_conversion(&state_db, &job, token.clone()) => result,
        () = token.cancelled() => Err(ApiError::from(AnoleError::new(
            ErrorCode::ExecutionFailed,
            anole_core::Stage::Execute,
            format!("conversion timed out after {} seconds", timeout.as_secs()),
            "Retry with a smaller file, or ask the operator to raise the timeout.",
        ))),
    };
    watchdog.abort();
    drop(permit);

    finish_web_job(&web, &job_id, outcome).await;
}

async fn run_conversion(
    state_db: &Path,
    job: &WebJob,
    cancellation: CancellationToken,
) -> Result<(PathBuf, Value), ApiError> {
    let mut request =
        plan_request_from_value(&job.request_fields, &job.target_format, &job.output_path)?;
    request.output_path = Some(job.output_path.clone());
    let (probe, plan, engine) = prepare_conversion(&job.input_path, &request)
        .await
        .map_err(ApiError::from)?;
    let mut store = open_job_store(state_db)?;
    let reports = ReportService::new(default_reports_directory(state_db));
    let result = ConversionService::run_prepared(
        &mut store,
        &reports,
        &probe,
        &plan,
        &engine,
        &plan.plan_hash,
        cancellation,
        |_| {},
    )
    .await
    .map_err(ApiError::from)?;
    let validation =
        serde_json::to_value(&result.report).unwrap_or_else(|_| json!({ "status": "unknown" }));
    Ok((result.output_path, validation))
}

async fn finish_web_job(web: &WebState, job_id: &str, outcome: Result<(PathBuf, Value), ApiError>) {
    // Privacy: the uploaded input served exactly this one conversion, so it
    // is deleted the moment the job reaches a terminal state.
    let upload_id = {
        let jobs = web.inner.jobs.lock().await;
        jobs.get(job_id).map(|job| job.upload_id.clone())
    };
    let stored_path = if let Some(upload_id) = upload_id {
        let mut uploads = web.inner.uploads.lock().await;
        uploads.remove(&upload_id).map(|record| record.stored_path)
    } else {
        None
    };
    if let Some(path) = stored_path {
        let _unused = std::fs::remove_file(path);
    }

    let ttl = web.config().ttl_secs;
    let mut jobs = web.inner.jobs.lock().await;
    let Some(job) = jobs.get_mut(job_id) else {
        return;
    };
    match outcome {
        Ok((output_path, validation)) => {
            job.state = WebJobState::Succeeded;
            job.output_path = output_path;
            job.validation = Some(validation);
            let archive = job.output_path.is_dir();
            job.download_name =
                download_name_for(&job.source_file_name, &job.target_format, archive);
            job.expires_at = now_plus(ttl);
        }
        Err(error) => {
            job.state = WebJobState::Failed;
            job.error = Some(error.into_json());
            job.expires_at = now_plus(ttl);
            remove_output_artifacts(job);
        }
    }
}

/// `GET /v1/jobs/{job_id}` — poll a queued/running/finished job.
///
/// # Errors
///
/// Returns a structured 404-style error for unknown job ids.
pub async fn get_job(
    State(state): State<AppState>,
    axum::extract::Path(job_id): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    let jobs = state.web().inner.jobs.lock().await;
    let job = jobs.get(&job_id).ok_or_else(|| {
        not_found(
            format!("unknown or expired job: {job_id}"),
            "Jobs expire one hour after they finish; run the conversion again.",
        )
    })?;
    Ok(Json(job_response(job)))
}

fn job_response(job: &WebJob) -> Value {
    let mut body = json!({
        "job_id": job.job_id,
        "upload_id": job.upload_id,
        "state": job.state.as_str(),
        "target_format": job.target_format,
        "created_at": unix_secs(job.created_at),
        "expires_at": unix_secs(job.expires_at),
        "download_url": Value::Null,
        "download_name": Value::Null,
        "is_directory_output": false,
        "validation": job.validation.clone().unwrap_or(Value::Null),
        "error": job.error.clone().unwrap_or(Value::Null),
    });
    if job.state == WebJobState::Succeeded {
        body["download_url"] = json!(format!("/v1/jobs/{}/download", job.job_id));
        body["download_name"] = json!(job.download_name);
        body["is_directory_output"] = json!(job.output_path.is_dir());
    }
    body
}

/// `GET /v1/jobs/{job_id}/download` — stream the converted artifact
/// (paged directory outputs are zipped on first download).
///
/// # Errors
///
/// Returns a structured error while the job is still running or on read failures.
///
/// # Panics
///
/// Panics only if the static `application/octet-stream` header value fails
/// to parse, which is a compile-time constant.
pub async fn download_job_output(
    State(state): State<AppState>,
    axum::extract::Path(job_id): axum::extract::Path<String>,
) -> Result<Response, ApiError> {
    let (output_path, archive_path, download_name) = {
        let jobs = state.web().inner.jobs.lock().await;
        let job = jobs.get(&job_id).ok_or_else(|| {
            not_found(
                format!("unknown or expired job: {job_id}"),
                "Jobs expire one hour after they finish; run the conversion again.",
            )
        })?;
        if job.state != WebJobState::Succeeded {
            return Err(invalid_input(
                format!(
                    "job {job_id} has not succeeded (state: {})",
                    job.state.as_str()
                ),
                "Wait until the job reports state `succeeded`, then download.",
            )
            .with_status(StatusCode::CONFLICT));
        }
        (
            job.output_path.clone(),
            job.archive_path.clone(),
            job.download_name.clone(),
        )
    };

    // Directory outputs (e.g. PDF → PNG pages) ship as a zip built lazily on
    // first download; the zip is cached until the TTL sweep deletes it.
    let source = if output_path.is_dir() {
        match archive_path {
            Some(archive) if archive.is_file() => archive,
            _ => {
                let archive = output_path.with_extension("zip");
                let zipped = tokio::task::spawn_blocking({
                    let directory = output_path.clone();
                    let archive = archive.clone();
                    move || zip_directory(&directory, &archive)
                })
                .await
                .map_err(|error| {
                    storage_failure(
                        "the output packaging task panicked",
                        Some(error.to_string()),
                    )
                })?;
                zipped?;
                if let Some(job) = state.web().inner.jobs.lock().await.get_mut(&job_id) {
                    job.archive_path = Some(archive.clone());
                }
                archive
            }
        }
    } else {
        output_path
    };

    if !source.is_file() {
        return Err(not_found(
            "the converted output is no longer available",
            "Outputs expire one hour after conversion; run the conversion again.",
        ));
    }
    let file = tokio::fs::File::open(&source).await.map_err(|error| {
        storage_failure(
            format!("cannot open the converted output: {}", source.display()),
            Some(error.to_string()),
        )
    })?;
    let stream = ReaderStream::new(file);
    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        "application/octet-stream"
            .parse()
            .expect("static header value"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        content_disposition(&download_name)
            .parse()
            .expect("ASCII header value with percent-encoded UTF-8 name"),
    );
    Ok(response)
}

/// `attachment; filename="fallback"; filename*=UTF-8''encoded` per RFC 6266.
fn content_disposition(download_name: &str) -> String {
    let ascii: String = download_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    for byte in download_name.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                encoded.push(byte as char);
            }
            _ => {
                use std::fmt::Write as _;
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

fn zip_directory(directory: &Path, archive: &Path) -> Result<(), ApiError> {
    let writer = std::fs::File::create(archive).map_err(|error| {
        storage_failure(
            format!("cannot create the output archive: {}", archive.display()),
            Some(error.to_string()),
        )
    })?;
    let mut zip = zip::ZipWriter::new(writer);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut pending = vec![(directory.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|error| {
            storage_failure(
                format!("cannot read the output directory: {}", dir.display()),
                Some(error.to_string()),
            )
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let archive_path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if path.is_dir() {
                zip.add_directory(format!("{archive_path}/"), options)
                    .map_err(|error| zip_error(&error))?;
                pending.push((path, archive_path));
            } else {
                zip.start_file(&archive_path, options)
                    .map_err(|error| zip_error(&error))?;
                let bytes = std::fs::read(&path).map_err(|error| {
                    storage_failure(
                        format!("cannot read the output file: {}", path.display()),
                        Some(error.to_string()),
                    )
                })?;
                zip.write_all(&bytes)
                    .map_err(|error| zip_error(&error.into()))?;
            }
        }
    }
    zip.finish().map_err(|error| zip_error(&error))?;
    Ok(())
}

fn zip_error(error: &zip::result::ZipError) -> ApiError {
    storage_failure(
        "cannot package the directory output",
        Some(error.to_string()),
    )
}

/// Static SPA fallback for unknown GET paths when `ANOLE_WEB_DIR` is set.
pub async fn spa_fallback(State(state): State<AppState>, uri: Uri) -> Response {
    let Some(static_dir) = state.web().config().static_dir.clone() else {
        return not_found("no such route", "Check the API path.").into_response();
    };
    let path = uri.path();
    if path == "/health" || path == "/openapi.json" || path.starts_with("/v1/") {
        return not_found("no such API route", "Check the API path.").into_response();
    }
    let Some(relative) = safe_relative_path(path) else {
        return not_found("invalid path", "Check the URL.").into_response();
    };
    let candidate = static_dir.join(&relative);
    if relative.as_os_str().is_empty() || !candidate.is_file() {
        // SPA history fallback: any unknown page route re-serves index.html.
        return serve_static_file(&static_dir.join("index.html")).await;
    }
    serve_static_file(&candidate).await
}

async fn serve_static_file(path: &Path) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mime = mime_for_extension(
                path.extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default(),
            );
            let immutable = path
                .parent()
                .and_then(|parent| parent.file_name())
                .is_some_and(|segment| segment == "assets");
            let mut response = Response::new(Body::from(bytes));
            let headers = response.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                mime.parse().expect("static header value"),
            );
            headers.insert(
                header::CACHE_CONTROL,
                if immutable {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-cache"
                }
                .parse()
                .expect("static header value"),
            );
            response
        }
        Err(_) => not_found(
            "the web app is not installed on this server",
            "Set ANOLE_WEB_DIR to the built SPA directory.",
        )
        .into_response(),
    }
}

fn safe_relative_path(path: &str) -> Option<PathBuf> {
    // URL 路径按相对语义解析：跳过前导 RootDir（Windows 上 "/x" 的
    // components 也会产生 RootDir）与 "."；拒绝盘符前缀与任何 ".."，
    // 防穿越。空相对路径（"/"）由调用方落到 index.html。
    let mut relative = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::RootDir | Component::CurDir => {}
            Component::Prefix(_) | Component::ParentDir => return None,
        }
    }
    Some(relative)
}

fn mime_for_extension(extension: &str) -> &'static str {
    match extension {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
