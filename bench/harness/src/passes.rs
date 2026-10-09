//! The pass loop of `bench run`: N full passes over the case × runner
//! matrix, each pass in a fresh shuffled order, each pass bracketed by
//! a calibration reading.
//!
//! Interleaving passes samples the machine's drift across time rather
//! than N back-to-back runs of one cell; shuffling the order inside a
//! pass keeps a slow drift from landing on the same cells every pass;
//! the calibration binary, run between every two cells, says whether
//! the machine moved under a cell (`machine_probe`).

use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::bench::{self, RunOutcome};
use crate::case::Case;
use crate::machine_probe::{CELL_REDOS, Probe, REDO_PAUSE, Reading, is_clean};
use crate::report::Calibration;
use crate::runner::Runner;

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

impl Passes<'_> {
    /// Runs every pass and returns one aggregated outcome per cell, in
    /// case-major order, plus each pass's machine record. A cell runs
    /// again while a probe next to it reads the machine as busy
    /// (`machine_probe`); the record says which cells did and whether
    /// any stayed bracketed by a busy probe.
    pub fn run(&self) -> Result<(Vec<RunOutcome>, Vec<Calibration>)> {
        let nr = self.runners.len();
        let ncell = self.cases.len() * nr;
        let mut acc: Vec<Vec<RunOutcome>> = (0..ncell).map(|_| Vec::new()).collect();
        // Compile is deterministic across passes for the same (case,
        // runner), so passes 2+ reuse the first accepted compile_ms; the
        // compile itself still runs to produce the artifact.
        let mut compile_cache: Vec<Option<f64>> = vec![None; ncell];
        let mut probe = match &self.calibration_bin {
            Some(b) => Some(Probe::new(b.clone())?),
            None => {
                eprintln!(
                    "→ no calibration binary (c runner or its compiler missing); passes run unprobed"
                );
                None
            }
        };
        // every accepted cell's two bracketing probes, judged again at the
        // end against the run's final floor
        let mut brackets: Vec<Vec<(String, f64, f64)>> = Vec::new();
        let mut calibration = Vec::new();
        let mut shuffle = Shuffle(self.seed | 1);
        for pass in 1..=self.runs {
            let mut rec = PassRecord::new(pass);
            let mut before = match probe.as_mut() {
                Some(p) => Some(rec.note(p.read()?)),
                None => None,
            };
            let mut pass_brackets = Vec::new();
            for idx in shuffle.order(ncell) {
                let (ci, ri) = (idx / nr, idx % nr);
                let (c, r) = (&self.cases[ci], &self.runners[ri]);
                let label = format!("{} × {}", c.name, r.name);
                if self.runs > 1 {
                    eprintln!("→ {label} (pass {pass}/{})", self.runs);
                } else {
                    eprintln!("→ {label}");
                }
                let mut tries = 0;
                let outcome = loop {
                    let outcome = bench::run_one(
                        c,
                        r,
                        self.work_dir,
                        self.workspace,
                        compile_cache[idx],
                        pass,
                    )?;
                    let (Some(p), Some(pre)) = (probe.as_mut(), before) else {
                        break outcome;
                    };
                    let post = rec.note(p.read()?);
                    before = Some(post);
                    if p.is_clean(&pre) && p.is_clean(&post) {
                        pass_brackets.push((label.clone(), pre.ms, post.ms));
                        break outcome;
                    }
                    if tries == CELL_REDOS {
                        eprintln!(
                            "⚠ {label}: still bracketed by a busy probe after {tries} redos \
                             ({:.3} / {:.3} ms, floor {:.3}); pass {pass} is contaminated",
                            pre.ms,
                            post.ms,
                            p.floor_ms()
                        );
                        rec.dirty.push(label.clone());
                        break outcome;
                    }
                    tries += 1;
                    eprintln!(
                        "⚠ {label}: probe {:.3} / {:.3} ms against floor {:.3}; again in {} s",
                        pre.ms,
                        post.ms,
                        p.floor_ms(),
                        REDO_PAUSE.as_secs()
                    );
                    if !rec.redone.contains(&label) {
                        rec.redone.push(label.clone());
                    }
                    std::thread::sleep(REDO_PAUSE);
                    before = Some(rec.note(p.read_after_pause()?));
                };
                if compile_cache[idx].is_none() {
                    compile_cache[idx] = outcome.compile_ms;
                }
                acc[idx].push(outcome);
            }
            brackets.push(pass_brackets);
            if probe.is_some() {
                calibration.push(rec);
            }
        }
        let calibration = match &probe {
            Some(p) => finish(calibration, &brackets, p.floor_ms()),
            None => Vec::new(),
        };
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

/// One pass's probe readings as they come.
struct PassRecord {
    pass: usize,
    readings: Vec<Reading>,
    redone: Vec<String>,
    dirty: Vec<String>,
}

impl PassRecord {
    fn new(pass: usize) -> Self {
        Self {
            pass,
            readings: Vec::new(),
            redone: Vec::new(),
            dirty: Vec::new(),
        }
    }

    fn note(&mut self, r: Reading) -> Reading {
        self.readings.push(r);
        r
    }
}

/// Judges every accepted cell's brackets again against the final floor:
/// a floor that fell after a cell was accepted can turn its probes busy.
fn finish(
    recs: Vec<PassRecord>,
    brackets: &[Vec<(String, f64, f64)>],
    floor_ms: f64,
) -> Vec<Calibration> {
    recs.into_iter()
        .zip(brackets)
        .map(|(mut rec, br)| {
            for (label, pre, post) in br {
                if !(is_clean(*pre, floor_ms) && is_clean(*post, floor_ms)) {
                    rec.dirty.push(label.clone());
                }
            }
            let first = rec.readings.first().copied();
            let last = rec.readings.last().copied();
            let ms: Vec<f64> = rec.readings.iter().map(|r| r.ms).collect();
            let loads: Vec<f64> = rec.readings.iter().filter_map(|r| r.load1).collect();
            let start_ms = first.map_or(0.0, |r| r.ms);
            let end_ms = last.map_or(0.0, |r| r.ms);
            Calibration {
                pass: rec.pass,
                start_ms,
                end_ms,
                start_instructions: first.and_then(|r| r.instructions),
                end_instructions: last.and_then(|r| r.instructions),
                unstable: ((end_ms - start_ms) / start_ms * 100.0).abs() > CALIBRATION_DRIFT_PCT,
                floor_ms,
                probes: ms.len(),
                probe_max_ms: ms.iter().copied().fold(0.0, f64::max),
                probes_busy: ms.iter().filter(|m| !is_clean(**m, floor_ms)).count(),
                load1_start: first.and_then(|r| r.load1),
                load1_max: loads.iter().copied().reduce(f64::max),
                load1_end: last.and_then(|r| r.load1),
                redone_cells: rec.redone,
                contaminated: !rec.dirty.is_empty(),
                dirty_cells: rec.dirty,
            }
        })
        .collect()
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
