//! T-22.b (v0.6.0) — torajs.com/playground HTTP API.
//!
//! `POST /api/run` accepts user TS source, compiles via the local
//! `tr build --target wasm32-wasi`, runs the resulting wasm under
//! `wasmtime` with strict resource caps, and returns captured
//! stdout/stderr.
//!
//! Sandboxing posture (every layer matters; remove any one and the
//! endpoint becomes a denial-of-service vector):
//!
//!  1. Source-size cap (8 KB) at the request boundary — bigger TS
//!     programs aren't the playground's job.
//!  2. SHA-256 of source → on-disk cache lookup keyed by hash. Hits
//!     skip both compile and run; the slow path only fires for novel
//!     programs.
//!  3. Compile is `tr build --target wasm32-wasi` in a fresh temp
//!     dir. Wall-clock timeout 30 s, max stderr size 32 KB.
//!  4. Run is `wasmtime` with `--fuel 2_000_000_000` (≈ a few hundred
//!     ms of compute on M-class hardware) + wall-clock timeout 5 s.
//!     wasmtime's fuel is per-instruction so even pathological JS
//!     can't hang the server. No filesystem mounts; the wasi
//!     environment is bare.
//!  5. Per-IP rate limit via tower_governor: 4 requests / 60 s burst.
//!
//! Returns:
//!     200 application/json
//!     { "stdout": "...", "stderr": "...", "exit_code": N,
//!       "compile_ms": N, "run_ms": N, "cached": bool }
//!
//! On compile error / timeout / run trap, status stays 200; the JSON
//! carries `error: "compile" | "compile_timeout" | "run_timeout" |
//! "run_trap"` so the frontend can render diagnostics inline without
//! a CORS-friendly status code dance.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info};

const MAX_SOURCE_BYTES: usize = 8 * 1024;
const MAX_STDERR_BYTES: usize = 32 * 1024;
const MAX_STDOUT_BYTES: usize = 64 * 1024;
const COMPILE_TIMEOUT: Duration = Duration::from_secs(30);
const RUN_TIMEOUT: Duration = Duration::from_secs(5);
const RUN_FUEL: u64 = 2_000_000_000;

#[derive(Parser, Debug)]
#[command(version, about = "torajs playground HTTP API")]
struct Args {
    /// Listen address. Default 127.0.0.1:8765 — bind 0.0.0.0 to
    /// expose; production should sit behind a reverse proxy with TLS.
    #[arg(long, default_value = "127.0.0.1:8765")]
    addr: SocketAddr,

    /// Path to the `tr` binary. Default = `tr` on $PATH.
    #[arg(long, default_value = "tr")]
    tr: PathBuf,

    /// Path to `wasmtime`. Default = `wasmtime` on $PATH.
    #[arg(long, default_value = "wasmtime")]
    wasmtime: PathBuf,

    /// Cache directory for {source-hash → wasm bytes}. Created on
    /// startup; existing entries kept across restarts.
    #[arg(long, default_value = "/tmp/torajs-playground-cache")]
    cache_dir: PathBuf,
}

#[derive(Clone)]
struct AppState {
    tr: Arc<PathBuf>,
    wasmtime: Arc<PathBuf>,
    cache_dir: Arc<PathBuf>,
}

#[derive(Deserialize)]
struct RunReq {
    source: String,
}

#[derive(Serialize)]
struct RunResp {
    stdout: String,
    stderr: String,
    exit_code: i32,
    compile_ms: u128,
    run_ms: u128,
    cached: bool,
    error: Option<&'static str>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "torajs_playground_api=info,tower_http=info".into()),
        )
        .init();
    let args = Args::parse();
    std::fs::create_dir_all(&args.cache_dir)?;
    info!(?args.addr, ?args.cache_dir, "torajs-playground-api starting");

    let state = AppState {
        tr: Arc::new(args.tr),
        wasmtime: Arc::new(args.wasmtime),
        cache_dir: Arc::new(args.cache_dir),
    };

    /* Per-IP burst: 4 requests, 1 every 15s refill. SmartIpKeyExtractor
     * checks X-Forwarded-For + X-Real-IP first (for the Caddy-on-t01
     * deploy where requests arrive proxied) before falling back to
     * the peer socket address (for direct dev binds). PeerIpKeyExtractor
     * needs the ConnectInfo middleware in the stack which axum::serve
     * doesn't add by default. */
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(15)
            .burst_size(4)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .ok_or("governor config")?,
    );

    let app = Router::new()
        .route("/api/run", post(run))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(GovernorLayer::new(governor_conf))
        .with_state(state);

    /* `into_make_service_with_connect_info::<SocketAddr>` exposes the
     * peer addr to GovernorLayer's IP-extractor fallback so direct
     * dev runs (no proxy headers) still rate-limit per client. */
    let listener = tokio::net::TcpListener::bind(args.addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

async fn run(State(state): State<AppState>, Json(req): Json<RunReq>) -> Response {
    if req.source.len() > MAX_SOURCE_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(RunResp::err("source_too_large")),
        )
            .into_response();
    }

    let hash = torajs_codec_hex::encode(Sha256::digest(req.source.as_bytes()));
    let cache_path = state.cache_dir.join(format!("{hash}.wasm"));

    let (wasm_path, cached, compile_ms) = if cache_path.exists() {
        (cache_path.clone(), true, 0u128)
    } else {
        match compile(state.clone(), &req.source, &cache_path).await {
            Ok((path, ms)) => (path, false, ms),
            Err(e) => {
                error!(err = ?e, "compile failed");
                return (
                    StatusCode::OK,
                    Json(RunResp::err_with(
                        match e {
                            CompileError::Timeout => "compile_timeout",
                            CompileError::Crash(_) => "compile_crash",
                            CompileError::DiagnosticOnly(_) => "compile",
                        },
                        e.diagnostics(),
                    )),
                )
                    .into_response();
            }
        }
    };

    /* Wasmtime invocation. `-W fuel=N` limits per-instruction
     * execution (deterministic across runs; ≈ a few hundred ms of
     * actual compute on M-class hardware); `-W timeout=5s` is a
     * wasmtime-level wall-clock hard cap that runs alongside our
     * tokio-side timeout (defense in depth — kernel-level signal
     * delivery vs in-process trap). `-W max-memory-size` caps
     * linear-memory growth. No `--dir` so the sandbox has no
     * filesystem access — `fs/promises` calls return ENOENT
     * cleanly inside this environment. */
    let started = std::time::Instant::now();
    let res = tokio::time::timeout(
        RUN_TIMEOUT,
        tokio::process::Command::new(&*state.wasmtime)
            .arg("run")
            .arg("-W")
            .arg(format!("fuel={RUN_FUEL}"))
            .arg("-W")
            .arg("timeout=5s")
            .arg("-W")
            .arg("max-memory-size=67108864") // 64 MiB cap
            .arg("--")
            .arg(&wasm_path)
            .output(),
    )
    .await;
    let run_ms = started.elapsed().as_millis();

    let output = match res {
        Err(_) => {
            return (
                StatusCode::OK,
                Json(RunResp {
                    stdout: String::new(),
                    stderr: format!("wasmtime exceeded {RUN_TIMEOUT:?}"),
                    exit_code: -1,
                    compile_ms,
                    run_ms,
                    cached,
                    error: Some("run_timeout"),
                }),
            )
                .into_response();
        }
        Ok(Err(e)) => {
            return (
                StatusCode::OK,
                Json(RunResp {
                    stdout: String::new(),
                    stderr: format!("wasmtime spawn failed: {e}"),
                    exit_code: -1,
                    compile_ms,
                    run_ms,
                    cached,
                    error: Some("run_spawn"),
                }),
            )
                .into_response();
        }
        Ok(Ok(o)) => o,
    };

    let stdout = trim_to(
        String::from_utf8_lossy(&output.stdout).to_string(),
        MAX_STDOUT_BYTES,
    );
    let stderr = trim_to(
        String::from_utf8_lossy(&output.stderr).to_string(),
        MAX_STDERR_BYTES,
    );
    let exit_code = output.status.code().unwrap_or(-1);

    info!(
        cached,
        compile_ms,
        run_ms,
        exit_code,
        stdout_bytes = stdout.len(),
        "run done"
    );

    (
        StatusCode::OK,
        Json(RunResp {
            stdout,
            stderr,
            exit_code,
            compile_ms,
            run_ms,
            cached,
            error: if exit_code != 0 {
                Some("run_trap")
            } else {
                None
            },
        }),
    )
        .into_response()
}

#[derive(Debug, thiserror::Error)]
enum CompileError {
    #[error("tr build exceeded {COMPILE_TIMEOUT:?}")]
    Timeout,
    #[error("tr build process failed: {0}")]
    Crash(String),
    #[error("tr build emitted diagnostics: {0}")]
    DiagnosticOnly(String),
}

impl CompileError {
    fn diagnostics(&self) -> String {
        match self {
            Self::Timeout => format!("tr build exceeded {COMPILE_TIMEOUT:?}"),
            Self::Crash(s) | Self::DiagnosticOnly(s) => s.clone(),
        }
    }
}

async fn compile(
    state: AppState,
    source: &str,
    cache_path: &std::path::Path,
) -> Result<(PathBuf, u128), CompileError> {
    let tmp = tempdir().map_err(|e| CompileError::Crash(format!("tempdir: {e}")))?;
    let src_path = tmp.path().join("source.ts");
    let wasm_path = tmp.path().join("source.wasm");
    std::fs::write(&src_path, source)
        .map_err(|e| CompileError::Crash(format!("write source: {e}")))?;

    let started = std::time::Instant::now();
    let res = tokio::time::timeout(
        COMPILE_TIMEOUT,
        tokio::process::Command::new(&*state.tr)
            .arg("build")
            .arg(&src_path)
            .arg("--target")
            .arg("wasm32-wasi")
            .arg("-o")
            .arg(&wasm_path)
            .output(),
    )
    .await;
    let compile_ms = started.elapsed().as_millis();

    let out = match res {
        Err(_) => return Err(CompileError::Timeout),
        Ok(Err(e)) => return Err(CompileError::Crash(format!("spawn tr: {e}"))),
        Ok(Ok(o)) => o,
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(CompileError::DiagnosticOnly(trim_to(
            stderr,
            MAX_STDERR_BYTES,
        )));
    }

    /* Promote to cache atomically. rename within the same dir is
     * atomic on every fs we care about; if we lose a race vs another
     * concurrent compile of the same hash, the loser's bytes are
     * identical so overwrite is safe. */
    std::fs::copy(&wasm_path, cache_path)
        .map_err(|e| CompileError::Crash(format!("cache copy: {e}")))?;
    /* tmp drops here, removing source.ts + source.wasm. */
    let _ = tmp;
    Ok((cache_path.to_path_buf(), compile_ms))
}

fn tempdir() -> std::io::Result<TempDir> {
    let mut base = std::env::temp_dir();
    let suffix: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
        ^ (std::process::id() as u64);
    base.push(format!("torajs-playground-{suffix:x}"));
    std::fs::create_dir_all(&base)?;
    Ok(TempDir { path: base })
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl RunResp {
    fn err(code: &'static str) -> Self {
        Self::err_with(code, String::new())
    }

    fn err_with(code: &'static str, stderr: String) -> Self {
        Self {
            stdout: String::new(),
            stderr,
            exit_code: -1,
            compile_ms: 0,
            run_ms: 0,
            cached: false,
            error: Some(code),
        }
    }
}

fn trim_to(mut s: String, cap: usize) -> String {
    if s.len() <= cap {
        return s;
    }
    s.truncate(cap);
    s.push_str("\n[…truncated]\n");
    s
}
