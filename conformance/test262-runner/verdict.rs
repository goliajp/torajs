//! tr execution + outcome classification for the test262 runner.
//!
//! Three judgment paths (takagi 2026-06-13 — "bun 没过我们也要试",
//! a bun failure is never by itself a reason to skip):
//!
//! - **oracle** — bun passed; tr must match exit 0 + stdout byte-equal
//!   (the legacy path).
//! - **negative** — the case's frontmatter declares an EXPECTED error
//!   (phase parse/early/resolution/runtime). bun is not consulted at
//!   all: the frontmatter is the oracle. tr passes by rejecting in
//!   the matching phase. `not yet supported:` / `import error:`
//!   rejects do NOT count as a pass (tr didn't *recognize* the error,
//!   it just lacks the feature) — those stay `incompatible`
//!   (`negative-unsupported`). Compile-time `type error:` on a
//!   runtime-phase case is recorded as `negative-static-reject`:
//!   tr's static typing legitimately hoists many spec runtime
//!   TypeErrors to compile time, which is a by-design divergence,
//!   not a bug.
//! - **no-oracle** — positive case but bun itself failed. test262
//!   positive cases self-validate through the assert harness, so
//!   tr exit 0 = pass (`PassNoOracle`); a tr failure classifies
//!   through the same subset-boundary table with a `no-oracle:`
//!   kind prefix so the breakdown stays attributable.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// `kill(2)` — sent with a negated pid to signal a whole process group.
/// tr AOT-compiles each case to a native binary it then spawns
/// (`torajs-run-new-*`); killing only the direct child (`Child::kill`)
/// orphaned that grandchild to init, where an infinite-loop or
/// runaway-allocation case kept a core pegged and memory growing forever.
/// On a shared runner that starved the host into a watchdog-timeout panic
/// and a reboot loop (mini, 2026-07-23). `torajs-test262` is a dev-only
/// harness; declaring the one libc call inline keeps its zero-dep build.
const SIGKILL: i32 = 9;
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// Per-case verdict. `Pass` = oracle-matched; `PassNoOracle` /
/// `PassNegative` = self-validated (see module doc).
#[derive(Debug, Clone)]
pub enum Outcome {
    Pass,
    PassNoOracle,
    PassNegative,
    Bug { kind: String, msg: String },
    Incompatible { kind: String, msg: String },
    HarnessError { msg: String },
}

/// Run tr on `tmp_path` with the 30s hang guard. tr's compile
/// pipeline shouldn't take that long even on huge cases; a 30s
/// outlier almost always means an O(n^2) pass or an infinite loop.
/// tr AOT cache stays enabled (content-keyed; byte-identical to the
/// no-cache outcome).
///
/// tr runs as the leader of its own process group so the guard can
/// signal the group (`kill(-pgid)`) — tr **and** the native binary it
/// AOT-spawns — rather than tr alone. Killing only tr orphaned the
/// grandchild, which on a hang kept running unbounded (see `SIGKILL`).
pub fn run_tr(tr_bin: &Path, tmp_path: &Path) -> Result<std::process::Output, Outcome> {
    let mut tr_proc = match Command::new(tr_bin)
        .args(["run", &tmp_path.to_string_lossy()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Err(Outcome::HarnessError {
                msg: format!("tr spawn: {e}"),
            });
        }
    };
    // Group leader's pid is the pgid; negate it to target the whole group.
    let pgid = tr_proc.id() as i32;
    let timeout = std::time::Duration::from_secs(30);
    let started = Instant::now();
    // Exponential backoff: most tr runs finish in tens of ms so a flat
    // 50ms sleep wastes tens of ms per fast case (× 53k case × 8 worker
    // = measurable sweep wall). Start at 0.5ms (catches fast cases nearly
    // idle-free), double up to a 20ms cap. Mirrors conformance runner
    // `conformance/runner/exec.rs` pattern.
    let mut backoff_us: u64 = 500;
    loop {
        match tr_proc.try_wait() {
            Ok(Some(_status)) => {
                return tr_proc
                    .wait_with_output()
                    .map_err(|e| Outcome::HarnessError {
                        msg: format!("tr output: {e}"),
                    });
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    // Kill the whole group so the AOT-spawned native child
                    // dies with tr instead of being orphaned to init.
                    unsafe { kill(-pgid, SIGKILL) };
                    let _ = tr_proc.wait();
                    return Err(Outcome::Incompatible {
                        kind: "tr-timeout".to_string(),
                        msg: format!("tr timed out after {}s", timeout.as_secs()),
                    });
                }
                std::thread::sleep(std::time::Duration::from_micros(backoff_us));
                backoff_us = (backoff_us * 2).min(20_000);
            }
            Err(e) => {
                return Err(Outcome::HarnessError {
                    msg: format!("tr wait: {e}"),
                });
            }
        }
    }
}

/// Subset-boundary stderr prefixes — tr deliberately rejecting
/// out-of-subset syntax / surface.
pub fn incompat_kind(first_line: &str) -> Option<&'static str> {
    if first_line.starts_with("lex error:") {
        Some("lex error")
    } else if first_line.starts_with("parse error:") {
        Some("parse error")
    } else if first_line.starts_with("type error:") {
        Some("type error")
    } else if first_line.starts_with("not yet supported:") {
        Some("not yet supported")
    } else if first_line.starts_with("import error:") {
        Some("import error")
    } else if first_line.starts_with("compile error:") {
        Some("compile error")
    } else {
        None
    }
}

/// Compile-time rejects that count as tr *recognizing* an error
/// (negative parse-phase pass), as opposed to lacking the feature.
fn is_recognized_compile_reject(kind: &str) -> bool {
    matches!(
        kind,
        "lex error" | "parse error" | "type error" | "compile error"
    )
}

fn first_stderr_line(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .next()
        .unwrap_or("(no stderr)")
        .to_string()
}

/// The line a BUG is attributed to.
///
/// tr prints its compile diagnostics before it runs the program, and
/// `warning: unknown identifier <n>` is a routine one: a case that
/// reads a deliberately-undeclared name (every `assert.throws(
/// ReferenceError, () => { u; })`) emits it and then fails for some
/// completely different reason. Recording the FIRST line put 6193 of
/// the bug rows under that warning with their real cause invisible —
/// a measurement device reporting its own noise. The classification
/// above still reads the first line, where a compile reject always
/// is; only the recorded message prefers the first `error:` line.
fn failure_line(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find(|l| l.trim_start().starts_with("error:"))
        .map(str::to_string)
        .unwrap_or_else(|| first_stderr_line(out))
}

/// Judge a `negative:` case — the expected-error phase is the oracle.
pub fn judge_negative(phase: &str, expected_type: &str, out: &std::process::Output) -> Outcome {
    if out.status.success() {
        return Outcome::Bug {
            kind: "negative-expected-error-but-passed".to_string(),
            msg: format!("expected {phase}-phase {expected_type}, tr exited 0"),
        };
    }
    let first_line = first_stderr_line(out);
    let compile_kind = incompat_kind(&first_line);
    match phase {
        "parse" | "early" => match compile_kind {
            Some(k) if is_recognized_compile_reject(k) => Outcome::PassNegative,
            Some(_) => Outcome::Incompatible {
                kind: "negative-unsupported".to_string(),
                msg: first_line,
            },
            // Runtime-stage failure on a parse-phase case: tr
            // accepted source it should have rejected.
            None => Outcome::Bug {
                kind: "negative-phase-mismatch".to_string(),
                msg: format!(
                    "expected {phase}-phase reject, got runtime failure: {}",
                    failure_line(out)
                ),
            },
        },
        "runtime" => match compile_kind {
            // Failed at runtime as expected (no compile-stage prefix).
            None => Outcome::PassNegative,
            Some("not yet supported") | Some("import error") => Outcome::Incompatible {
                kind: "negative-unsupported".to_string(),
                msg: first_line,
            },
            // Static typing hoisting a spec runtime error to compile
            // time — by-design divergence, attributable bucket.
            Some(_) => Outcome::Incompatible {
                kind: "negative-static-reject".to_string(),
                msg: first_line,
            },
        },
        // Module resolution phase. Only the resolver's own
        // ResolveExport verdict counts as a pass — the marker is the
        // `module resolution failure` message `static_resolution.rs`
        // composes under the CLI's `import error:` prefix. Every
        // other reject (path not found, unsupported surface, a
        // checker error the case tripped for the wrong reason) stays
        // incompatible: crediting them would be metric inflation —
        // the reject exists but not via the semantics under test.
        _ => match compile_kind {
            Some("import error") if first_line.contains("module resolution failure") => {
                Outcome::PassNegative
            }
            Some(_) | None => Outcome::Incompatible {
                kind: "negative-unsupported".to_string(),
                msg: format!("phase {phase}: {first_line}"),
            },
        },
    }
}

/// `flags: [async]` completion marker (test262 doneprintHandle.js
/// protocol): the harness's `$DONE` prints this on success. An async
/// case's process exit code is meaningless — a dropped promise chain
/// exits 0 without ever completing — so pass judgment keys on the
/// marker, never on exit status alone.
pub const ASYNC_COMPLETE: &str = "Test262:AsyncTestComplete";
/// `$DONE(error)` failure marker prefix.
pub const ASYNC_FAILURE: &str = "Test262:AsyncTestFailure";

/// True iff stdout signals a clean async completion: the complete
/// marker present and no failure marker (a failed-then-completed run
/// is still a failure).
pub fn async_completed(stdout: &[u8]) -> bool {
    let s = String::from_utf8_lossy(stdout);
    s.contains(ASYNC_COMPLETE) && !s.contains(ASYNC_FAILURE)
}

/// Judge a positive case bun itself failed on: the assert harness
/// self-validates, so exit 0 is a pass. Failures classify through
/// the subset table with a `no-oracle:` prefix. Async cases
/// additionally require the `$DONE` completion marker — exit 0 with
/// no marker means the promise machinery never drove the callback,
/// which is a bug, not a pass (no-metric-inflation).
pub fn judge_no_oracle(out: &std::process::Output, is_async: bool) -> Outcome {
    if out.status.success() && is_async && !async_completed(&out.stdout) {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let (kind, msg) = match stdout.lines().find(|l| l.starts_with(ASYNC_FAILURE)) {
            Some(fail_line) => ("no-oracle:async-failure", fail_line.to_string()),
            None => (
                "no-oracle:async-incomplete",
                format!("exit 0 without {ASYNC_COMPLETE}"),
            ),
        };
        return Outcome::Bug {
            kind: kind.to_string(),
            msg,
        };
    }
    if out.status.success() {
        return Outcome::PassNoOracle;
    }
    let first_line = first_stderr_line(out);
    if let Some(kind) = incompat_kind(&first_line) {
        return Outcome::Incompatible {
            kind: format!("no-oracle:{kind}"),
            msg: first_line,
        };
    }
    let kind = if let Some(code) = out.status.code() {
        format!("no-oracle:exit {code}")
    } else {
        "no-oracle:killed".to_string()
    };
    Outcome::Bug {
        kind,
        msg: failure_line(out),
    }
}

/// Judge a positive case with a live bun oracle (legacy path).
///
/// `is_async` splits the one bucket this path used to collapse. An
/// async case that exits 0 with stdout unlike bun's can be three
/// different things — an assertion that threw and printed the failure
/// marker, a promise chain that never drove `$DONE` at all, or output
/// that differs for some ordinary reason — and all three arrived here
/// as `stdout-mismatch` carrying the `(no stderr)` placeholder,
/// because the failure marker goes to STDOUT and `failure_line` reads
/// stderr.
///
/// [`judge_no_oracle`] has always made this split, with the
/// no-metric-inflation note above it. The same question was being
/// answered two ways depending on whether bun happened to complete,
/// which made the collapsed bucket unusable as a progress measure:
/// 146 cases sat in it at rotation 696 and nothing could say how many
/// were failing assertions rather than silent exits.
///
/// The verdict itself does not move — `Pass` is still byte equality,
/// and every case that was a `Bug` stays one. Only the bucket gets
/// its name back.
pub fn judge_oracle(out: &std::process::Output, bun_stdout: &[u8], is_async: bool) -> Outcome {
    if out.status.success() && out.stdout == bun_stdout {
        return Outcome::Pass;
    }
    let first_line = first_stderr_line(out);
    if let Some(kind) = incompat_kind(&first_line) {
        return Outcome::Incompatible {
            kind: kind.to_string(),
            msg: first_line,
        };
    }
    if !out.status.success() {
        let kind = match out.status.code() {
            Some(code) => format!("exit {code}"),
            None => "killed".to_string(),
        };
        return Outcome::Bug {
            kind,
            msg: failure_line(out),
        };
    }
    if is_async && !async_completed(&out.stdout) {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let (kind, msg) = match stdout.lines().find(|l| l.starts_with(ASYNC_FAILURE)) {
            Some(fail_line) => ("async-failure", fail_line.to_string()),
            None => (
                "async-incomplete",
                format!("exit 0 without {ASYNC_COMPLETE}"),
            ),
        };
        return Outcome::Bug {
            kind: kind.to_string(),
            msg,
        };
    }
    Outcome::Bug {
        kind: "stdout-mismatch".to_string(),
        msg: failure_line(out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    fn out(code: i32, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn bug_message_skips_a_leading_warning() {
        // The routine shape: a compile warning about a name the
        // program deliberately leaves undeclared, then the real
        // failure. The row must carry the failure.
        let o = out(
            1,
            "warning: unknown identifier `u`\nerror: uncaught TypeError: nope\n",
        );
        assert!(matches!(
            judge_oracle(&o, b"", false),
            Outcome::Bug { msg, .. } if msg == "error: uncaught TypeError: nope"
        ));
    }

    #[test]
    fn bug_message_falls_back_to_the_first_line() {
        let o = out(139, "");
        assert!(
            matches!(judge_oracle(&o, b"", false), Outcome::Bug { msg, .. } if msg == "(no stderr)")
        );
    }

    // The three shapes the oracle path used to collapse into one
    // `stdout-mismatch` / `(no stderr)` row. The marker goes to
    // STDOUT, which is why the stderr-reading fallback had nothing to
    // say about any of them.
    #[test]
    fn async_assertion_failure_names_itself() {
        let o = out_stdout(0, "Test262:AsyncTestFailure:Test262Error: wrong value\n");
        assert!(matches!(
            judge_oracle(&o, ASYNC_COMPLETE.as_bytes(), true),
            Outcome::Bug { kind, msg }
                if kind == "async-failure" && msg.contains("wrong value")
        ));
    }

    #[test]
    fn async_never_completing_names_itself() {
        let o = out_stdout(0, "");
        assert!(matches!(
            judge_oracle(&o, ASYNC_COMPLETE.as_bytes(), true),
            Outcome::Bug { kind, msg }
                if kind == "async-incomplete" && msg.contains(ASYNC_COMPLETE)
        ));
    }

    // The split must not reach a case that is not async, or a plain
    // stdout difference would start reporting itself as a promise
    // that never settled.
    #[test]
    fn a_sync_stdout_difference_is_still_stdout_mismatch() {
        let o = out_stdout(0, "7\n");
        assert!(matches!(
            judge_oracle(&o, b"8\n", false),
            Outcome::Bug { kind, .. } if kind == "stdout-mismatch"
        ));
    }

    // An async case that DID complete and still differs is an
    // ordinary mismatch — the marker is present, so neither async
    // bucket applies.
    #[test]
    fn a_completed_async_case_that_differs_is_not_bucketed_as_async() {
        let o = out_stdout(0, &format!("extra\n{ASYNC_COMPLETE}\n"));
        assert!(matches!(
            judge_oracle(&o, ASYNC_COMPLETE.as_bytes(), true),
            Outcome::Bug { kind, .. } if kind == "stdout-mismatch"
        ));
    }

    #[test]
    fn negative_parse_recognized_reject_passes() {
        let o = out(1, "parse error: unexpected token");
        assert!(matches!(
            judge_negative("parse", "SyntaxError", &o),
            Outcome::PassNegative
        ));
    }

    #[test]
    fn negative_parse_unsupported_is_incompatible() {
        let o = out(3, "not yet supported: labeled statement");
        assert!(matches!(
            judge_negative("parse", "SyntaxError", &o),
            Outcome::Incompatible { kind, .. } if kind == "negative-unsupported"
        ));
    }

    #[test]
    fn negative_runtime_static_reject_attributed() {
        let o = out(1, "type error: cannot read property of undefined");
        assert!(matches!(
            judge_negative("runtime", "TypeError", &o),
            Outcome::Incompatible { kind, .. } if kind == "negative-static-reject"
        ));
    }

    #[test]
    fn negative_runtime_failure_passes() {
        let o = out(1, "TypeError: boom");
        assert!(matches!(
            judge_negative("runtime", "TypeError", &o),
            Outcome::PassNegative
        ));
    }

    #[test]
    fn negative_exit_zero_is_bug() {
        let o = out(0, "");
        assert!(matches!(
            judge_negative("parse", "SyntaxError", &o),
            Outcome::Bug { kind, .. } if kind == "negative-expected-error-but-passed"
        ));
    }

    #[test]
    fn no_oracle_exit_zero_passes() {
        let o = out(0, "");
        assert!(matches!(judge_no_oracle(&o, false), Outcome::PassNoOracle));
    }

    #[test]
    fn no_oracle_subset_reject_prefixed() {
        let o = out(3, "not yet supported: generators");
        assert!(matches!(
            judge_no_oracle(&o, false),
            Outcome::Incompatible { kind, .. } if kind == "no-oracle:not yet supported"
        ));
    }

    fn out_stdout(code: i32, stdout: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn async_exit_zero_without_marker_is_bug() {
        let o = out_stdout(0, "");
        assert!(matches!(
            judge_no_oracle(&o, true),
            Outcome::Bug { kind, .. } if kind == "no-oracle:async-incomplete"
        ));
    }

    #[test]
    fn async_failure_marker_is_bug() {
        let o = out_stdout(0, "Test262:AsyncTestFailure:Test262Error: boom\n");
        assert!(matches!(
            judge_no_oracle(&o, true),
            Outcome::Bug { kind, .. } if kind == "no-oracle:async-failure"
        ));
    }

    #[test]
    fn async_complete_marker_passes() {
        let o = out_stdout(0, "Test262:AsyncTestComplete\n");
        assert!(matches!(judge_no_oracle(&o, true), Outcome::PassNoOracle));
    }

    #[test]
    fn async_complete_after_failure_still_fails() {
        let o = out_stdout(0, "Test262:AsyncTestFailure:X\nTest262:AsyncTestComplete\n");
        assert!(!async_completed(&o.stdout));
    }
}
