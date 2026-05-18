//! v0.3 #5 L-6 — LSP latency bench. Synthesizes a 1 K-line .ts
//! fixture, spawns `tr lsp` as a subprocess, drives the JSON-RPC
//! protocol over stdio, and reports per-operation round-trip
//! latencies (P50 / P95 / max) for:
//!   - didOpen → publishDiagnostics (cold-start typecheck)
//!   - hover (warm typecheck path)
//!   - definition (source-text scan path)
//!
//! Budget: < 50 ms P95 on 1 K-line file (per RFC 20260505-lsp-server-skeleton.md).
//! If over budget, add per-text-hash caching of (expr_types, errors)
//! in lsp.rs so repeated hover requests against unchanged text skip
//! the pipeline.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const HOVER_REQUESTS: usize = 50;
const DEFINITION_REQUESTS: usize = 50;
const DEFAULT_FIXTURE_LINES: usize = 1000;

pub fn run(self_exe: &std::path::Path) -> std::process::ExitCode {
    let lines = std::env::args()
        .nth(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_FIXTURE_LINES);
    let fixture = synthesize_fixture(lines);
    let line_count = fixture.lines().count();
    println!("fixture: {} lines, {} bytes", line_count, fixture.len());

    let mut child = match spawn_server(self_exe) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "lsp-bench: failed to spawn `{} lsp`: {e}",
                self_exe.display()
            );
            return std::process::ExitCode::from(1);
        }
    };
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));

    if let Err(e) = run_bench(&mut stdin, &mut stdout, &fixture) {
        eprintln!("lsp-bench: {e}");
        let _ = child.kill();
        let _ = child.wait();
        return std::process::ExitCode::from(1);
    }

    let _ = child.kill();
    let _ = child.wait();
    std::process::ExitCode::SUCCESS
}

fn spawn_server(self_exe: &std::path::Path) -> std::io::Result<Child> {
    Command::new(self_exe)
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
}

fn run_bench(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    fixture: &str,
) -> Result<(), String> {
    // -- initialize handshake --
    let init_id = 1i64;
    send_request(
        stdin,
        init_id,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": serde_json::Value::Null,
            "capabilities": {},
        }),
    )?;
    expect_response(stdout, init_id)?;
    send_notification(stdin, "initialized", serde_json::json!({}))?;

    // -- didOpen → publishDiagnostics (cold typecheck) --
    let uri = "file:///tmp/torajs-lsp-bench-1k.ts";
    let cold_start = Instant::now();
    send_notification(
        stdin,
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": "typescript",
                "version": 1,
                "text": fixture,
            }
        }),
    )?;
    let _diag = wait_for_notification(stdout, "textDocument/publishDiagnostics")?;
    let cold_ms = cold_start.elapsed().as_secs_f64() * 1000.0;
    println!("didOpen → publishDiagnostics (cold): {cold_ms:.2} ms");

    // -- hover requests at varied positions --
    let positions = sample_positions(fixture, HOVER_REQUESTS);
    let mut hover_times = Vec::with_capacity(positions.len());
    let mut hover_hits = 0usize;
    for (i, (line, character)) in positions.iter().enumerate() {
        let id = 100 + i as i64;
        let start = Instant::now();
        send_request(
            stdin,
            id,
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )?;
        let resp = expect_response(stdout, id)?;
        hover_times.push(start.elapsed());
        if !resp.get("result").map(|v| v.is_null()).unwrap_or(true) {
            hover_hits += 1;
        }
    }

    // -- definition requests at varied positions --
    let mut def_times = Vec::with_capacity(positions.len());
    let mut def_hits = 0usize;
    for (i, (line, character)) in positions.iter().take(DEFINITION_REQUESTS).enumerate() {
        let id = 1000 + i as i64;
        let start = Instant::now();
        send_request(
            stdin,
            id,
            "textDocument/definition",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )?;
        let resp = expect_response(stdout, id)?;
        def_times.push(start.elapsed());
        if !resp.get("result").map(|v| v.is_null()).unwrap_or(true) {
            def_hits += 1;
        }
    }

    print_stats("hover     ", &hover_times, hover_hits);
    print_stats("definition", &def_times, def_hits);

    // Budget check: P95 hover under 50 ms is the gate.
    let hover_p95 = percentile(&hover_times, 95);
    let target = Duration::from_millis(50);
    if hover_p95 <= target {
        println!(
            "verdict: hover P95 = {:.2} ms ≤ 50 ms budget — UNDER",
            hover_p95.as_secs_f64() * 1000.0
        );
    } else {
        println!(
            "verdict: hover P95 = {:.2} ms > 50 ms budget — OVER",
            hover_p95.as_secs_f64() * 1000.0
        );
    }

    // -- shutdown --
    let shutdown_id = 9999i64;
    send_request(stdin, shutdown_id, "shutdown", serde_json::Value::Null)?;
    let _ = expect_response(stdout, shutdown_id);
    send_notification(stdin, "exit", serde_json::Value::Null)?;
    Ok(())
}

/// Synthesize FIXTURE_LINES of varied TypeScript: a mix of function
/// declarations, type aliases, classes, and let bindings, so the
/// typechecker has real work to do (not just a no-op file).
fn synthesize_fixture(lines: usize) -> String {
    let mut out = String::with_capacity(lines * 50);
    // Repeating pattern: each block is 10 lines and contains
    // function + type + class + let. Repeat to reach line count.
    let mut i: usize = 0;
    while out.lines().count() < lines {
        let n = i;
        out.push_str(&format!("// Block {n}\n"));
        out.push_str(&format!("type T{n} = {{ x: i64, y: i64 }}\n"));
        out.push_str(&format!("function add{n}(a: i64, b: i64): i64 {{\n"));
        out.push_str(&format!("  let s: i64 = a + b\n"));
        out.push_str(&format!("  return s\n"));
        out.push_str("}\n");
        out.push_str(&format!("function mul{n}(a: i64, b: i64): i64 {{\n"));
        out.push_str(&format!("  return a * b\n"));
        out.push_str("}\n");
        out.push_str(&format!("let v{n}: i64 = add{n}(1, 2) + mul{n}(3, 4)\n"));
        i += 1;
        if i > lines {
            break;
        }
    }
    out
}

/// Sample N (line, character) cursor positions across the file. Lands
/// on the FIRST identifier-shaped token that's preceded by `= ` (i.e.
/// a binding's RHS expression), which check.rs reliably types and
/// scan_top_level_symbols can resolve. Falls back to first ident on
/// the line if no `= ` is found, so non-`let` lines still contribute.
fn sample_positions(fixture: &str, count: usize) -> Vec<(u32, u32)> {
    let lines: Vec<&str> = fixture.lines().collect();
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let interesting: Vec<(usize, u32)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let t = l.trim_start();
            // Only top-level `let v...` lines: their RHS Call is
            // typed by the bare-parse path the LSP uses (function-
            // body locals aren't tracked without desugars).
            if !l.starts_with("let v") {
                let _ = t;
                return None;
            }
            // Prefer position right after the first `= `.
            let cursor_start = l.find("= ").map(|p| p + 2).unwrap_or(0);
            let bytes = l.as_bytes();
            let mut j = cursor_start;
            while j < bytes.len() && !is_ident(bytes[j] as char) {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            let start = j;
            while j < bytes.len() && is_ident(bytes[j] as char) {
                j += 1;
            }
            // Cursor mid-identifier.
            Some((i, ((start + j) / 2) as u32))
        })
        .collect();
    if interesting.is_empty() {
        return Vec::new();
    }
    let stride = (interesting.len() / count.max(1)).max(1);
    interesting
        .into_iter()
        .step_by(stride)
        .take(count)
        .map(|(li, col)| (li as u32, col))
        .collect()
}

fn print_stats(label: &str, times: &[Duration], hits: usize) {
    if times.is_empty() {
        println!("{label}: no samples");
        return;
    }
    let mut sorted = times.to_vec();
    sorted.sort();
    let p50 = sorted[sorted.len() / 2];
    let p95 = sorted[sorted.len() * 95 / 100];
    let max = *sorted.last().unwrap();
    let sum: Duration = sorted.iter().sum();
    let mean = sum / sorted.len() as u32;
    println!(
        "{label}: n={} hits={} mean={:.2}ms p50={:.2}ms p95={:.2}ms max={:.2}ms",
        sorted.len(),
        hits,
        mean.as_secs_f64() * 1000.0,
        p50.as_secs_f64() * 1000.0,
        p95.as_secs_f64() * 1000.0,
        max.as_secs_f64() * 1000.0,
    );
}

fn percentile(times: &[Duration], p: usize) -> Duration {
    if times.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted = times.to_vec();
    sorted.sort();
    sorted[(sorted.len() * p / 100).min(sorted.len() - 1)]
}

// ---------- LSP framing ----------

fn send_request(
    stdin: &mut ChildStdin,
    id: i64,
    method: &str,
    params: serde_json::Value,
) -> Result<(), String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    write_message(stdin, &body)
}

fn send_notification(
    stdin: &mut ChildStdin,
    method: &str,
    params: serde_json::Value,
) -> Result<(), String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    write_message(stdin, &body)
}

fn write_message(stdin: &mut ChildStdin, body: &serde_json::Value) -> Result<(), String> {
    let s = serde_json::to_string(body).map_err(|e| format!("serialize: {e}"))?;
    let header = format!("Content-Length: {}\r\n\r\n", s.len());
    stdin
        .write_all(header.as_bytes())
        .and_then(|_| stdin.write_all(s.as_bytes()))
        .and_then(|_| stdin.flush())
        .map_err(|e| format!("write: {e}"))
}

fn read_message(stdout: &mut BufReader<ChildStdout>) -> Result<serde_json::Value, String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = stdout
            .read_line(&mut line)
            .map_err(|e| format!("read header: {e}"))?;
        if n == 0 {
            return Err("server closed stream".into());
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length: ") {
            content_length = rest.parse::<usize>().ok();
        }
    }
    let len = content_length.ok_or("missing Content-Length header")?;
    let mut buf = vec![0u8; len];
    stdout
        .read_exact(&mut buf)
        .map_err(|e| format!("read body: {e}"))?;
    serde_json::from_slice(&buf).map_err(|e| format!("parse json: {e}"))
}

fn expect_response(
    stdout: &mut BufReader<ChildStdout>,
    id: i64,
) -> Result<serde_json::Value, String> {
    // Drain notifications (e.g. publishDiagnostics) until we see the
    // matching response.
    loop {
        let msg = read_message(stdout)?;
        if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
            return Ok(msg);
        }
    }
}

fn wait_for_notification(
    stdout: &mut BufReader<ChildStdout>,
    method: &str,
) -> Result<serde_json::Value, String> {
    loop {
        let msg = read_message(stdout)?;
        if msg.get("method").and_then(|v| v.as_str()) == Some(method) {
            return Ok(msg);
        }
    }
}
