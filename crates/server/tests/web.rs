//! Integration tests for the web-track endpoints (uploads, plan preview,
//! queued jobs, downloads, SPA hosting) layered onto the unchanged local
//! API surface.

use std::path::PathBuf;
use std::time::Duration;

use anole_server::routes::{AppState, build_router};
use anole_server::web::{WebConfig, WebState};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tempfile::TempDir;
use tower::ServiceExt;

struct WebTestServer {
    router: Router,
    dir: TempDir,
    #[allow(dead_code)]
    state_db: PathBuf,
}

fn web_test_server(config: WebConfig) -> WebTestServer {
    let dir = tempfile::tempdir().expect("temp dir");
    let state_db = dir.path().join("jobs.sqlite3");
    let config = WebConfig {
        root_dir: if config.root_dir.as_os_str().is_empty() {
            dir.path().join("web")
        } else {
            config.root_dir
        },
        ..config
    };
    let state = AppState::with_web_config(&state_db, WebState::new(config));
    WebTestServer {
        router: build_router(state),
        dir,
        state_db,
    }
}

fn default_web_test_server() -> WebTestServer {
    web_test_server(WebConfig {
        root_dir: PathBuf::new(),
        max_upload_bytes: anole_server::web::DEFAULT_MAX_UPLOAD_BYTES,
        ttl_secs: anole_server::web::DEFAULT_TTL_SECS,
        conversion_timeout_secs: anole_server::web::DEFAULT_CONVERSION_TIMEOUT_SECS,
        max_concurrent_conversions: anole_server::web::DEFAULT_MAX_CONCURRENT_CONVERSIONS,
        static_dir: None,
    })
}

fn multipart_body(boundary: &str, filename: &str, content: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>, String, String) {
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let disposition = response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = axum::body::to_bytes(response.into_body(), 64 << 20)
        .await
        .expect("body");
    (status, bytes.to_vec(), content_type, disposition)
}

async fn post_multipart(
    router: &Router,
    uri: &str,
    filename: &str,
    content: &[u8],
) -> (StatusCode, serde_json::Value) {
    let boundary = "anole-test-boundary";
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(multipart_body(boundary, filename, content)))
        .expect("multipart request");
    let (status, bytes, _, _) = send(router, request).await;
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, body)
}

async fn post_json_raw(
    router: &Router,
    uri: &str,
    body: String,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("json request");
    let (status, bytes, _, _) = send(router, request).await;
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, body)
}

async fn post_json(
    router: &Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    post_json_raw(router, uri, body.to_string()).await
}

async fn get_json(router: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("get");
    let (status, bytes, _, _) = send(router, request).await;
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, body)
}

async fn upload_fixture(router: &Router, content: &[u8]) -> String {
    let (status, body) = post_multipart(router, "/v1/uploads", "data.json", content).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    body["upload_id"].as_str().expect("upload_id").to_owned()
}

async fn wait_for_terminal_job(router: &Router, job_id: &str) -> serde_json::Value {
    // 转换本身是毫秒级内置引擎，但 CI 单核 runner 上并行测试会抢占
    // 调度；窗口放宽到 30s（仍远小于 120s 转换超时，足够暴露真死锁）。
    for _ in 0..240 {
        let (status, body) = get_json(router, &format!("/v1/jobs/{job_id}")).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        let state = body["state"].as_str().unwrap_or_default();
        if state == "succeeded" || state == "failed" {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    panic!("job {job_id} did not reach a terminal state in time");
}

#[tokio::test]
async fn web_flow_uploads_plans_converts_and_downloads() {
    let server = default_web_test_server();
    let upload_id = upload_fixture(&server.router, br#"[{"id":1,"name":"alpha"}]"#).await;

    // Capabilities for the uploaded input exist and include the builtin
    // structured route.
    let (status, capabilities) = get_json(
        &server.router,
        &format!("/v1/uploads/{upload_id}/capabilities"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {capabilities}");
    assert_eq!(capabilities["routes"]["yaml"]["available"], true);

    // Plan preview (not executed) reports the snake_case plan surface.
    let (status, plan_body) = post_json(
        &server.router,
        &format!("/v1/uploads/{upload_id}/plan"),
        serde_json::json!({ "target_format": "yaml" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {plan_body}");
    assert_eq!(plan_body["plan"]["target_format"], "yaml");
    assert!(
        plan_body["plan_hash"]
            .as_str()
            .is_some_and(|h| !h.is_empty()),
        "missing plan_hash: {plan_body}"
    );

    // Queued conversion reaches succeeded with a ValidationReport.
    let (status, job) = post_json(
        &server.router,
        "/v1/jobs",
        serde_json::json!({ "upload_id": upload_id, "target_format": "yaml" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "body: {job}");
    let job_id = job["job_id"].as_str().expect("job_id").to_owned();
    let terminal = wait_for_terminal_job(&server.router, &job_id).await;
    assert_eq!(terminal["state"], "succeeded", "job failed: {terminal}");
    assert!(
        terminal["validation"]["status"].as_str().is_some(),
        "missing validation report: {terminal}"
    );
    assert_eq!(
        terminal["download_url"],
        format!("/v1/jobs/{job_id}/download")
    );
    assert_eq!(terminal["download_name"], "data.converted.yaml");

    // The download streams the converted artifact with attachment headers.
    let request = Request::builder()
        .uri(format!("/v1/jobs/{job_id}/download"))
        .body(Body::empty())
        .expect("download request");
    let (status, bytes, content_type, disposition) = send(&server.router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "application/octet-stream");
    assert!(
        disposition.contains("attachment"),
        "disposition: {disposition}"
    );
    let text = String::from_utf8_lossy(&bytes).to_string();
    assert!(text.contains("alpha"), "unexpected output: {text}");

    // Privacy contract: the input is deleted once the conversion finished.
    let uploads_dir = server.dir.path().join("web").join("uploads");
    let count = std::fs::read_dir(&uploads_dir).map_or(0, |entries| entries.flatten().count());
    assert_eq!(
        count, 0,
        "uploaded input should be deleted after conversion"
    );

    // The consumed upload ticket is gone.
    let (status, _) = get_json(
        &server.router,
        &format!("/v1/uploads/{upload_id}/capabilities"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upload_rejects_unsupported_extension_with_structured_error() {
    let server = default_web_test_server();
    let (status, body) = post_multipart(&server.router, "/v1/uploads", "tool.exe", b"MZ").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert_eq!(body["code"], "INPUT_INVALID");
}

#[tokio::test]
async fn upload_rejects_oversized_files() {
    let server = web_test_server(WebConfig {
        root_dir: PathBuf::new(),
        max_upload_bytes: 8,
        ttl_secs: anole_server::web::DEFAULT_TTL_SECS,
        conversion_timeout_secs: anole_server::web::DEFAULT_CONVERSION_TIMEOUT_SECS,
        max_concurrent_conversions: anole_server::web::DEFAULT_MAX_CONCURRENT_CONVERSIONS,
        static_dir: None,
    });
    let payload = vec![b'x'; 64];
    let (status, body) = post_multipart(&server.router, "/v1/uploads", "big.json", &payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert_eq!(body["code"], "INPUT_INVALID");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("limit")),
        "expected a size-limit diagnostic: {body}"
    );
}

#[tokio::test]
async fn upload_rejects_path_fields_injected_into_plan_requests() {
    let server = default_web_test_server();
    let upload_id = upload_fixture(&server.router, br#"[{"id":1}]"#).await;
    let (status, body) = post_json(
        &server.router,
        &format!("/v1/uploads/{upload_id}/plan"),
        serde_json::json!({
            "target_format": "yaml",
            "input_path": "/etc/passwd",
            "output_path": "/tmp/escape.yaml",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    // The planned output stays inside the server-owned web jobs directory.
    let planned_output = body["plan"]["output_path"].as_str().unwrap_or_default();
    assert!(
        planned_output.replace('\\', "/").contains("web/jobs/"),
        "planned output escaped the web root: {planned_output}"
    );
}

#[tokio::test]
async fn unknown_upload_and_job_ids_map_to_structured_404s() {
    let server = default_web_test_server();
    let (status, body) = post_json(
        &server.router,
        "/v1/uploads/00000000-0000-0000-0000-000000000000/plan",
        serde_json::json!({ "target_format": "yaml" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {body}");
    assert_eq!(body["code"], "INPUT_INVALID");

    let (status, body) = get_json(
        &server.router,
        "/v1/jobs/00000000-0000-0000-0000-000000000000",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {body}");
}

#[tokio::test]
async fn job_rejects_missing_target_and_unknown_upload() {
    let server = default_web_test_server();
    let upload_id = upload_fixture(&server.router, br#"[{"id":1}]"#).await;
    let (status, body) = post_json(
        &server.router,
        "/v1/jobs",
        serde_json::json!({ "upload_id": upload_id }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert_eq!(body["code"], "INPUT_INVALID");

    let (status, body) = post_json(
        &server.router,
        "/v1/jobs",
        serde_json::json!({ "upload_id": "nope", "target_format": "yaml" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {body}");
}

#[tokio::test]
async fn download_before_success_conflicts() {
    let server = default_web_test_server();
    let upload_id = upload_fixture(&server.router, br#"[{"id":1}]"#).await;
    let (status, body) = post_json(
        &server.router,
        "/v1/jobs",
        serde_json::json!({ "upload_id": upload_id, "target_format": "yaml" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "body: {body}");
    let job_id = body["job_id"].as_str().expect("job_id").to_owned();
    // The conversion may finish quickly; either CONFLICT (not done yet) or
    // OK (done) is acceptable, but never a 404/5xx.
    let request = Request::builder()
        .uri(format!("/v1/jobs/{job_id}/download"))
        .body(Body::empty())
        .expect("download request");
    let (status, _, _, _) = send(&server.router, request).await;
    assert!(
        status == StatusCode::CONFLICT || status == StatusCode::OK,
        "unexpected early-download status: {status}"
    );
}

#[tokio::test]
async fn ttl_sweep_hard_deletes_expired_uploads() {
    let dir = tempfile::tempdir().expect("temp dir");
    let web = WebState::new(WebConfig {
        root_dir: dir.path().join("web"),
        max_upload_bytes: anole_server::web::DEFAULT_MAX_UPLOAD_BYTES,
        ttl_secs: 0,
        conversion_timeout_secs: anole_server::web::DEFAULT_CONVERSION_TIMEOUT_SECS,
        max_concurrent_conversions: anole_server::web::DEFAULT_MAX_CONCURRENT_CONVERSIONS,
        static_dir: None,
    });
    let router = build_router(AppState::with_web_config(
        dir.path().join("jobs.sqlite3"),
        web.clone(),
    ));
    let (status, body) = post_multipart(&router, "/v1/uploads", "data.json", b"[1]").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let upload_id = body["upload_id"].as_str().expect("upload_id").to_owned();
    let uploads_dir = dir.path().join("web").join("uploads");
    assert_eq!(
        std::fs::read_dir(&uploads_dir).map_or(0, |entries| entries.flatten().count()),
        1
    );

    web.sweep_expired().await;
    assert_eq!(
        std::fs::read_dir(&uploads_dir).map_or(0, |entries| entries.flatten().count()),
        0,
        "expired upload should be hard-deleted"
    );
    let (status, _) = get_json(&router, &format!("/v1/uploads/{upload_id}/capabilities")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn spa_hosting_serves_index_and_api_404s() {
    let dir = tempfile::tempdir().expect("temp dir");
    let static_dir = dir.path().join("spa");
    std::fs::create_dir_all(static_dir.join("assets")).expect("assets dir");
    std::fs::write(static_dir.join("index.html"), "<html>anole-spa</html>").expect("index");
    std::fs::write(static_dir.join("assets").join("app.js"), "console.log(1)").expect("asset");

    let state_dir = tempfile::tempdir().expect("state dir");
    let state = AppState::with_web_config(
        state_dir.path().join("jobs.sqlite3"),
        WebState::new(WebConfig {
            root_dir: state_dir.path().join("web"),
            max_upload_bytes: anole_server::web::DEFAULT_MAX_UPLOAD_BYTES,
            ttl_secs: anole_server::web::DEFAULT_TTL_SECS,
            conversion_timeout_secs: anole_server::web::DEFAULT_CONVERSION_TIMEOUT_SECS,
            max_concurrent_conversions: anole_server::web::DEFAULT_MAX_CONCURRENT_CONVERSIONS,
            static_dir: Some(static_dir),
        }),
    );
    let router = build_router(state);

    // Root serves the SPA entry.
    let request = Request::builder()
        .uri("/")
        .body(Body::empty())
        .expect("root");
    let (status, bytes, content_type, _) = send(&router, request).await;
    assert_eq!(status, StatusCode::OK, "body: {bytes:?}");
    assert!(content_type.starts_with("text/html"), "ct: {content_type}");
    assert!(
        String::from_utf8_lossy(&bytes).contains("anole-spa"),
        "body: {bytes:?}"
    );

    // Hash-named assets are immutable-cached.
    let request = Request::builder()
        .uri("/assets/app.js")
        .body(Body::empty())
        .expect("asset");
    let (status, _, content_type, _) = send(&router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/javascript"));

    // Unknown page routes fall back to index.html (SPA history mode).
    let request = Request::builder()
        .uri("/some/history/route")
        .body(Body::empty())
        .expect("history route");
    let (status, bytes, content_type, _) = send(&router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"));
    assert!(String::from_utf8_lossy(&bytes).contains("anole-spa"));

    // Path traversal is refused.
    let request = Request::builder()
        .uri("/../etc/passwd")
        .body(Body::empty())
        .expect("traversal");
    let (status, _, _, _) = send(&router, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Unknown API routes stay structured 404 JSON, not index.html.
    let request = Request::builder()
        .uri("/v1/nope")
        .body(Body::empty())
        .expect("api 404");
    let (status, bytes, content_type, _) = send(&router, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(content_type.starts_with("application/json"));
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json 404");
    assert_eq!(body["code"], "INPUT_INVALID");
}

#[tokio::test]
async fn spa_hosting_disabled_returns_404_for_pages() {
    let server = default_web_test_server();
    let request = Request::builder()
        .uri("/")
        .body(Body::empty())
        .expect("root");
    let (status, _, _, _) = send(&server.router, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn openapi_documents_web_endpoints() {
    let server = default_web_test_server();
    let (status, body) = get_json(&server.router, "/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    for path in [
        "/health",
        "/openapi.json",
        "/v1/plan",
        "/v1/convert",
        "/v1/capabilities",
        "/v1/uploads",
        "/v1/uploads/{upload_id}/capabilities",
        "/v1/uploads/{upload_id}/plan",
        "/v1/jobs",
        "/v1/jobs/{job_id}",
        "/v1/jobs/{job_id}/download",
    ] {
        assert!(
            body["paths"][path].is_object(),
            "openapi missing path {path}"
        );
    }
}
