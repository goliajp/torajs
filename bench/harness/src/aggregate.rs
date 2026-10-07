//! Folding the passes of one (case, runtime) into its results row.

use crate::bench::{RunOutcome, Status};

/// hardev bench B1b — median of a slice (avg of the two middle values
/// for even length). `None` for empty input.
fn median(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

/// Median absolute deviation — robust spread, far less spike-sensitive
/// than stddev (a single mac thermal/load spike barely moves it).
fn mad(xs: &[f64], med: f64) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let dev: Vec<f64> = xs.iter().map(|x| (x - med).abs()).collect();
    median(&dev)
}

/// hardev bench B1b — fold N passes of the SAME (case, runtime) into
/// one row: `run_ms` = median across passes, `run_stddev_ms` = MAD
/// (robust spread), `compile_ms` = median, `max_rss_kb` = the largest, `artifact_bytes` = the
/// shared value if all identical else the median (the small ±N-byte
/// drift seen run-to-run is benign linker nondeterminism, already
/// handled conservatively by `bench compare`). `status` is the worst
/// observed (Failed > Skipped > Ok) so a single failing pass is never
/// hidden by aggregation. Caller guarantees `outcomes` is non-empty
/// and homogeneous in (case, runtime).
pub fn aggregate(outcomes: Vec<RunOutcome>) -> RunOutcome {
    let mut base = outcomes[0].clone();

    base.status = if outcomes.iter().any(|o| o.status == Status::Failed) {
        Status::Failed
    } else if outcomes.iter().any(|o| o.status == Status::Skipped) {
        Status::Skipped
    } else {
        Status::Ok
    };

    let run: Vec<f64> = outcomes.iter().filter_map(|o| o.run_ms).collect();
    base.run_ms = median(&run);
    base.run_stddev_ms = base.run_ms.and_then(|m| mad(&run, m));
    base.max_rss_kb = outcomes.iter().filter_map(|o| o.max_rss_kb).max();

    let comp: Vec<f64> = outcomes.iter().filter_map(|o| o.compile_ms).collect();
    base.compile_ms = median(&comp);

    base.run_samples_ms = outcomes
        .iter()
        .flat_map(|o| o.run_samples_ms.iter().copied())
        .collect();
    base.run_min_ms = outcomes
        .iter()
        .filter_map(|o| o.run_min_ms)
        .reduce(f64::min);
    let med_f = |get: fn(&RunOutcome) -> Option<f64>| {
        let xs: Vec<f64> = outcomes.iter().filter_map(get).collect();
        median(&xs)
    };
    let med_u = |get: fn(&RunOutcome) -> Option<u64>| {
        let xs: Vec<f64> = outcomes.iter().filter_map(get).map(|v| v as f64).collect();
        median(&xs).map(|m| m.round() as u64)
    };
    base.user_ms = med_f(|o| o.user_ms);
    base.system_ms = med_f(|o| o.system_ms);
    base.instructions = med_u(|o| o.instructions);
    base.cycles = med_u(|o| o.cycles);
    base.pinstructions = med_u(|o| o.pinstructions);
    base.pcycles = med_u(|o| o.pcycles);
    base.phys_footprint_kb = outcomes.iter().filter_map(|o| o.phys_footprint_kb).max();
    base.pageins = med_u(|o| o.pageins);
    base.diskio_read_bytes = med_u(|o| o.diskio_read_bytes);
    base.diskio_written_bytes = med_u(|o| o.diskio_written_bytes);
    base.minor_faults = med_u(|o| o.minor_faults);
    base.major_faults = med_u(|o| o.major_faults);
    base.ctx_switches = med_u(|o| o.ctx_switches);

    let arts: Vec<u64> = outcomes.iter().filter_map(|o| o.artifact_bytes).collect();
    base.artifact_bytes = if arts.is_empty() {
        None
    } else if arts.iter().all(|&a| a == arts[0]) {
        Some(arts[0])
    } else {
        let af: Vec<f64> = arts.iter().map(|&a| a as f64).collect();
        median(&af).map(|m| m.round() as u64)
    };

    // artifact_sha256: keep only when every pass produced the same
    // digest — a per-pass hash divergence means linker
    // nondeterminism, and publishing any single pass's digest would
    // make the next --vs precheck flag a phantom change.
    let hashes: Vec<&String> = outcomes
        .iter()
        .filter_map(|o| o.artifact_sha256.as_ref())
        .collect();
    base.artifact_sha256 = if !hashes.is_empty() && hashes.iter().all(|h| *h == hashes[0]) {
        Some(hashes[0].clone())
    } else {
        None
    };

    base.retries = outcomes
        .iter()
        .flat_map(|o| o.retries.iter().cloned())
        .collect();

    // surface the first error if the aggregate failed; clear it otherwise
    base.error = if base.status == Status::Failed {
        outcomes.iter().find_map(|o| o.error.clone())
    } else {
        None
    };
    base
}
