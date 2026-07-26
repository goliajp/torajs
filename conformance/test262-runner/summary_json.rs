//! Machine-readable sweep summary (`--json PATH`) and the two stamps
//! that go in it.
//!
//! The hardev dashboard's `snapshot.mjs` reads this file; every field
//! name here is part of that contract. Extracted from main.rs to keep
//! that file shrinking (it is over the file-size.md limit as known
//! debt — see the runner's own module docs).

use std::io::Write;
use std::path::Path;

/// Minimal hand-rolled JSON object writer (test262-runner is zero-dep).
/// Escapes `"` and `\` in strings; everything else assumed ASCII-safe.
pub fn write_summary_json(
    out: &Path,
    ran_at: &str,
    head_sha: &str,
    elapsed_sec: f64,
    workers: usize,
    limit: Option<usize>,
    total_cases: usize,
    ran: usize,
    pass: usize,
    pass_no_oracle: usize,
    pass_negative: usize,
    bug: usize,
    incompatible: usize,
    bun_fail: usize,
    harness_error: usize,
    in_scope: usize,
    tr_accepted: usize,
    pass_rate_in_scope: f64,
    pass_rate_tr_accepted: f64,
) -> std::io::Result<()> {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let limit_json = match limit {
        Some(n) => n.to_string(),
        None => "null".to_string(),
    };
    // `bunSkip` keeps its key for dashboard compatibility but now
    // counts bun failures that were still judged (no case is skipped).
    let body = format!(
        "{{\n  \"ranAt\": \"{ra}\",\n  \"headSha\": \"{hs}\",\n  \"elapsedSec\": {es:.2},\n  \"workers\": {w},\n  \"limit\": {lj},\n  \"totalCases\": {tc},\n  \"ran\": {ran},\n  \"pass\": {pass},\n  \"passNoOracle\": {pno},\n  \"passNegative\": {png},\n  \"passTotal\": {ptot},\n  \"bug\": {bug},\n  \"incompatible\": {inc},\n  \"bunSkip\": {bs},\n  \"harnessError\": {he},\n  \"inScope\": {is_},\n  \"trAccepted\": {ta},\n  \"passRateInScope\": {pris:.2},\n  \"passRateTrAccepted\": {prta:.2}\n}}\n",
        ra = esc(ran_at),
        hs = esc(head_sha),
        es = elapsed_sec,
        w = workers,
        lj = limit_json,
        tc = total_cases,
        ran = ran,
        pass = pass,
        pno = pass_no_oracle,
        png = pass_negative,
        ptot = pass + pass_no_oracle + pass_negative,
        bug = bug,
        inc = incompatible,
        bs = bun_fail,
        he = harness_error,
        is_ = in_scope,
        ta = tr_accepted,
        pris = pass_rate_in_scope,
        prta = pass_rate_tr_accepted,
    );
    let mut f = std::fs::File::create(out)?;
    f.write_all(body.as_bytes())?;
    Ok(())
}

/// Read git HEAD short sha from `.git/HEAD` chain, falling back to
/// `git rev-parse` if the chain doesn't resolve to a packed/loose ref.
/// Best-effort — returns "unknown" on any error since the JSON consumer
/// must tolerate it (cold-start invocations may not be in a git repo).
pub fn detect_head_sha() -> String {
    if let Ok(out) = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    "unknown".to_string()
}

/// RFC 3339 UTC timestamp without external deps. Uses libc gmtime.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Convert epoch seconds to civil date via Howard Hinnant's algorithm.
    let z = secs as i64;
    let days = z.div_euclid(86_400);
    let secs_of_day = z.rem_euclid(86_400);
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day / 60) % 60;
    let ss = secs_of_day % 60;
    let z_days = days + 719_468;
    let era = if z_days >= 0 {
        z_days
    } else {
        z_days - 146_096
    } / 146_097;
    let doe = (z_days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z",
        y = y,
        m = m,
        d = d,
        hh = hh,
        mm = mm,
        ss = ss,
    )
}
