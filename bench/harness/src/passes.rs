//! The pass loop of `bench run`: N full passes over the case × runner
//! matrix, each pass in a fresh shuffled order, each pass bracketed by
//! a calibration reading.
//!
//! Interleaving passes samples the machine's drift across time rather
//! than N back-to-back runs of one cell; shuffling the order inside a
//! pass keeps a slow drift from landing on the same cells every pass;
//! the calibration binary says whether the machine moved during a pass
//! at all.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::bench::{self, RunOutcome};
use crate::case::Case;
use crate::report::Calibration;
use crate::runner::Runner;
use crate::rusage::measure_runs;

/// A pass whose calibration readings differ by more than this ran on
/// a machine that changed under it.
const CALIBRATION_DRIFT_PCT: f64 = 3.0;

/// xorshift64*: enough to derandomise an order, and no dependency.
struct Shuffle(u64);

impl Shuffle {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn order(&mut self, n: usize) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            idx.swap(i, j);
        }
        idx
    }
}

pub struct Passes<'a> {
    pub cases: &'a [Case],
    pub runners: &'a [Runner],
    pub runs: usize,
    pub work_dir: &'a Path,
    pub workspace: &'a Path,
    pub seed: u64,
    /// Run command of the compiled calibration binary, args included;
    /// `None` skips calibration.
    pub calibration_bin: Option<String>,
}

/// Compiles the calibration program with the `c` runner's own compile
/// template and returns its run command, `args` appended. `None` when
/// the compiler is not there — the run goes on without calibration
/// rather than not at all.
pub fn build_calibration(
    c: &Runner,
    src: &Path,
    args: &[String],
    work_dir: &Path,
) -> Result<Option<String>> {
    let Some(template) = &c.compile else {
        return Ok(None);
    };
    let out = work_dir.join(bench::out_name("calibration", "c"));
    let ctx = crate::runner::TemplateContext {
        src: src.to_string_lossy().to_string(),
        out: out.to_string_lossy().to_string(),
        case: "calibration".to_string(),
        workspace: String::new(),
    };
    let cmd = ctx.substitute(template);
    let parts = crate::runner::split_cmd(&cmd);
    let compile_args: Vec<&str> = parts[1..].iter().map(String::as_str).collect();
    match crate::exec::spawn_output_bounded(&parts[0], &compile_args, &[], Duration::from_secs(120))
    {
        Ok(o) if o.status.success() => {
            let mut run = out.to_string_lossy().to_string();
            for a in args {
                run.push(' ');
                run.push_str(a);
            }
            Ok(Some(run))
        }
        _ => Ok(None),
    }
}

fn calibrate(bin: &str) -> Result<(f64, Option<u64>)> {
    let st = measure_runs(bin, 0, 1, &[], Duration::from_secs(60))?;
    Ok((st.median_ms, st.instructions))
}

impl Passes<'_> {
    /// Runs every pass and returns one aggregated outcome per cell, in
    /// case-major order, plus the calibration readings.
    pub fn run(&self) -> Result<(Vec<RunOutcome>, Vec<Calibration>)> {
        let nr = self.runners.len();
        let ncell = self.cases.len() * nr;
        let mut acc: Vec<Vec<RunOutcome>> = (0..ncell).map(|_| Vec::new()).collect();
        // Compile is deterministic across passes for the same (case,
        // runner), so passes 2+ reuse the first pass's compile_ms; the
        // compile itself still runs to produce the artifact.
        let mut compile_cache: Vec<Option<f64>> = vec![None; ncell];
        let mut calibration = Vec::new();
        let mut shuffle = Shuffle(self.seed | 1);
        for pass in 1..=self.runs {
            let start = match &self.calibration_bin {
                Some(b) => Some(calibrate(b)?),
                None => None,
            };
            for idx in shuffle.order(ncell) {
                let (ci, ri) = (idx / nr, idx % nr);
                let (c, r) = (&self.cases[ci], &self.runners[ri]);
                if self.runs > 1 {
                    eprintln!("→ {} × {} (pass {pass}/{})", c.name, r.name, self.runs);
                } else {
                    eprintln!("→ {} × {}", c.name, r.name);
                }
                let cached = compile_cache[idx];
                let outcome = bench::run_one(c, r, self.work_dir, self.workspace, cached)?;
                if cached.is_none()
                    && let Some(cm) = outcome.compile_ms
                {
                    compile_cache[idx] = Some(cm);
                }
                acc[idx].push(outcome);
            }
            if let (Some(b), Some((start_ms, start_instructions))) = (&self.calibration_bin, start)
            {
                let (end_ms, end_instructions) = calibrate(b)?;
                let unstable =
                    ((end_ms - start_ms) / start_ms * 100.0).abs() > CALIBRATION_DRIFT_PCT;
                if unstable {
                    eprintln!(
                        "⚠ pass {pass}: calibration moved {start_ms:.3} → {end_ms:.3} ms \
                         (> {CALIBRATION_DRIFT_PCT}%); the machine changed under this pass"
                    );
                }
                calibration.push(Calibration {
                    pass,
                    start_ms,
                    end_ms,
                    start_instructions,
                    end_instructions,
                    unstable,
                });
            }
        }
        let mut rows = Vec::with_capacity(ncell);
        for passes in acc {
            rows.push(if passes.len() > 1 {
                crate::report::aggregate(passes)
            } else {
                passes.into_iter().next().expect("at least one pass")
            });
        }
        Ok((rows, calibration))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_is_a_permutation_and_seeded() {
        let a = Shuffle(42 | 1).order(46);
        let b = Shuffle(42 | 1).order(46);
        assert_eq!(a, b);
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..46).collect::<Vec<_>>());
        assert_ne!(Shuffle(44 | 1).order(46), a);
    }
}
