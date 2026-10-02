//! Machine-readable sweep summary (`--json PATH`) and the two stamps
//! that go in it.
//!
//! The hardev dashboard's `snapshot.mjs` reads this file; every field
//! name here is part of that contract. Extracted from main.rs to keep
//! that file shrinking (it is over the file-size.md limit as known
//! debt — see the runner's own module docs).

use std::io::Write;
use std::path::Path;

/// One sweep's summary, in the field order the JSON writes.
pub struct Summary<'a> {
    pub ran_at: &'a str,
    pub head_sha: &'a str,
    pub elapsed_sec: f64,
    pub workers: usize,
    pub limit: Option<usize>,
    pub total_cases: usize,
    pub ran: usize,
    pub pass: usize,
    pub pass_no_oracle: usize,
    pub pass_negative: usize,
    pub bug: usize,
    pub incompatible: usize,
    pub bun_fail: usize,
    pub harness_error: usize,
    pub in_scope: usize,
    pub tr_accepted: usize,
    pub pass_rate_in_scope: f64,
    pub pass_rate_tr_accepted: f64,
}

/// Minimal hand-rolled JSON object writer (test262-runner is zero-dep).
/// Escapes `"` and `\` in strings; everything else assumed ASCII-safe.
pub fn write_summary_json(out: &Path, summary: &Summary) -> std::io::Result<()> {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let Summary {
        ran_at,
        head_sha,
        elapsed_sec,
        workers,
        limit,
        total_cases,
        ran,
        pass,
        pass_no_oracle,
        pass_negative,
        bug,
        incompatible,
        bun_fail,
        harness_error,
        in_scope,
        tr_accepted,
        pass_rate_in_scope,
        pass_rate_tr_accepted,
    } = *summary;
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

/// Best-effort provenance for the summary's `headSha`.
///
/// git answers a question about the *directory this runs in*, which is
/// not the question the field is asked: which source produced the `tr`
/// under test. The two come apart whenever the tree is not the tree
/// HEAD names — this project rsyncs its runner box, so that box's HEAD
/// can sit many commits behind the source actually being swept, and a
/// bare sha there is a confident lie. A tree that differs from HEAD is
/// therefore stamped `<sha>-dirty`: HEAD does not name this source, and
/// the reader is told so rather than shown a plausible commit.
///
/// Callers who know the real sha pass `--head-sha` and skip all of this.
/// Returns "unknown" on any error — a cold-start invocation need not be
/// in a git repo at all.
pub fn detect_head_sha() -> String {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let Some(sha) = git(&["rev-parse", "--short", "HEAD"]).filter(|s| !s.is_empty()) else {
        return "unknown".to_string();
    };
    match git(&["status", "--porcelain", "--untracked-files=no"]) {
        Some(st) if st.is_empty() => sha,
        Some(_) => format!("{sha}-dirty"),
        None => "unknown".to_string(),
    }
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
