//! One (case, runtime) row of a results file.

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RunOutcome {
    pub case: String,
    pub runtime: String,
    pub runtime_version: Option<String>,
    pub status: Status,
    pub compile_ms: Option<f64>,
    pub run_ms: Option<f64>,
    pub run_stddev_ms: Option<f64>,
    /// Peak resident set size of the run step, in KiB: the largest over
    /// every measured run. `None` when not measured.
    pub max_rss_kb: Option<u64>,
    /// Size in bytes of the compiled artifact, for runners that produce one.
    /// `None` for interpreted runners (bun/node/python; tr-interp).
    pub artifact_bytes: Option<u64>,
    /// SHA-256 (hex) of the compiled artifact's contents. The actual
    /// "machine code unchanged" signal: equal size does NOT imply
    /// equal code — Mach-O 16 KiB segment alignment absorbs small
    /// text-section deltas, so a size-only comparison reports
    /// "identical" across real codegen changes (observed: the Cluster
    /// E inliner firing on 8 bench cases with every artifact size
    /// unchanged). `None` for interpreted runners and for result
    /// files written before this field existed.
    pub artifact_sha256: Option<String>,
    pub stdout_match: Option<bool>,
    pub error: Option<String>,
    /// The run step's raw wall-clock samples (ms), every pass
    /// concatenated, so a reader can redo the statistics.
    #[serde(default)]
    pub run_samples_ms: Vec<f64>,
    pub run_min_ms: Option<f64>,
    /// Kernel resource account of the run step (macOS
    /// `proc_pid_rusage`): medians across runs, footprint the largest.
    pub user_ms: Option<f64>,
    pub system_ms: Option<f64>,
    pub instructions: Option<u64>,
    pub cycles: Option<u64>,
    pub pinstructions: Option<u64>,
    pub pcycles: Option<u64>,
    pub phys_footprint_kb: Option<u64>,
    pub pageins: Option<u64>,
    pub diskio_read_bytes: Option<u64>,
    pub diskio_written_bytes: Option<u64>,
    /// `wait4` account of the run step, medians across runs: minor
    /// and major page faults, voluntary + involuntary context switches.
    pub minor_faults: Option<u64>,
    pub major_faults: Option<u64>,
    pub ctx_switches: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Ok,
    Skipped,
    Failed,
}

impl RunOutcome {
    pub(crate) fn new(case: &str, runtime: &str) -> Self {
        Self {
            case: case.into(),
            runtime: runtime.into(),
            ..Self::default()
        }
    }

    pub(crate) fn skip(case: &str, runtime: &str, reason: String) -> Self {
        Self {
            status: Status::Skipped,
            error: Some(reason),
            ..Self::new(case, runtime)
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status != Status::Failed
    }
}
