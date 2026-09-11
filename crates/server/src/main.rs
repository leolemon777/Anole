//! Entry point for the `Anole` local `REST` API server (G-33).

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;

use anole_server::routes::{AppState, build_router};

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Err(error) = runtime.block_on(serve()) {
        eprintln!("anole-server: {error}");
        std::process::exit(1);
    }
}

async fn serve() -> Result<(), String> {
    let bind = parse_bind_address(std::env::args().skip(1))?;
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|error| format!("failed to bind {bind}: {error}"))?;
    let state = AppState::new(default_state_db());
    // Web track: hard-delete expired uploads/outputs once a minute.
    anole_server::web::spawn_ttl_sweeper(state.web().clone());
    let app = build_router(state);
    println!("anole-server listening on http://{bind}");
    // ConnectInfo powers the web track's per-IP admission checks.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|error| format!("server error: {error}"))
}

/// Parses an optional `--bind <addr>` flag; defaults to loopback only.
/// Container platforms (Render/HF Spaces) expose `PORT` instead, which
/// widens the default bind to all interfaces when set.
fn parse_bind_address<I>(args: I) -> Result<SocketAddr, String>
where
    I: Iterator<Item = String>,
{
    let mut bind = match std::env::var("PORT") {
        Ok(port) if !port.trim().is_empty() => format!("0.0.0.0:{}", port.trim()),
        _ => "127.0.0.1:8787".to_owned(),
    };
    let mut iter = args.peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--bind" => {
                bind = iter
                    .next()
                    .ok_or_else(|| "--bind requires a socket address".to_owned())?;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    bind.parse()
        .map_err(|error| format!("invalid bind address {bind}: {error}"))
}

fn default_state_db() -> PathBuf {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(root).join("Anole").join("jobs.sqlite3");
    }

    #[cfg(target_os = "macos")]
    if let Some(root) = std::env::var_os("HOME") {
        return PathBuf::from(root)
            .join("Library")
            .join("Application Support")
            .join("Anole")
            .join("jobs.sqlite3");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(root) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(root).join("anole").join("jobs.sqlite3");
    } else if let Some(root) = std::env::var_os("HOME") {
        return PathBuf::from(root)
            .join(".local")
            .join("state")
            .join("anole")
            .join("jobs.sqlite3");
    }

    PathBuf::from("anole-jobs.sqlite3")
}
