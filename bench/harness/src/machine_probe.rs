//! Watches the machine while a pass runs, so a reading taken on a busy
//! machine is measured again or thrown away instead of pooled.
//!
//! The calibration binary (gcd1m's C build, about 37 ms) runs between
//! every two cells. It does the same work every time, so its wall time
//! moves only when the machine does: another job, a frequency drop, a
//! scan. A cell counts only when the probes on both sides of it read
//! within `PROBE_CLEAN_PCT` of the fastest probe of the whole run; a
//! cell bracketed by a slow probe runs again after a pause, and a cell
//! still bracketed by one after `CELL_REDOS` tries marks its pass
//! contaminated, which fails the run.
//!
//! The system's one-minute load average is recorded at every probe
//! next to it. It lags the machine by about a minute and counts the
//! bench's own processes, so it is reported, not used to decide.

use std::time::Duration;

use anyhow::Result;

use crate::report::Calibration;
use crate::rusage::measure_runs;

/// A probe slower than the fastest probe of the run by more than this
/// says the machine was not the one the other readings ran on. A clean
/// run's probes spread under 3 %.
pub const PROBE_CLEAN_PCT: f64 = 5.0;

/// How many times a cell bracketed by a slow probe runs again.
pub const CELL_REDOS: usize = 3;

/// The pause before running a cell again, so a burst can end.
pub const REDO_PAUSE: Duration = Duration::from_secs(10);

/// Probes taken before the first pass to find the machine's floor.
const FLOOR_PROBES: usize = 5;

#[derive(Debug, Clone, Copy)]
pub struct Reading {
    pub ms: f64,
    pub instructions: Option<u64>,
    pub load1: Option<f64>,
}

pub struct Probe {
    bin: String,
    floor_ms: f64,
}

impl Probe {
    /// Runs the calibration binary `FLOOR_PROBES` times and keeps the
    /// fastest as the floor every later probe is judged against.
    pub fn new(bin: String) -> Result<Self> {
        let mut p = Self { bin, floor_ms: f64::INFINITY };
        for _ in 0..FLOOR_PROBES {
            p.read()?;
        }
        Ok(p)
    }

    pub fn read(&mut self) -> Result<Reading> {
        let st = measure_runs(&self.bin, 0, 1, &[], Duration::from_secs(60))?;
        self.floor_ms = self.floor_ms.min(st.median_ms);
        Ok(Reading { ms: st.median_ms, instructions: st.instructions, load1: load1() })
    }

    pub fn floor_ms(&self) -> f64 {
        self.floor_ms
    }

    pub fn is_clean(&self, r: &Reading) -> bool {
        is_clean(r.ms, self.floor_ms)
    }
}

pub fn is_clean(ms: f64, floor_ms: f64) -> bool {
    ms <= floor_ms * (1.0 + PROBE_CLEAN_PCT / 100.0)
}

/// False, and says so, when a pass kept a reading bracketed by a busy
/// probe: such a results file is not a measurement and is not pooled.
pub fn clean_run(cal: &[Calibration]) -> bool {
    let bad: Vec<usize> = cal.iter().filter(|c| c.contaminated).map(|c| c.pass).collect();
    if !bad.is_empty() {
        eprintln!("CONTAMINATED: pass {bad:?} kept a reading bracketed by a busy probe");
    }
    bad.is_empty()
}

unsafe extern "C" {
    fn getloadavg(loadavg: *mut f64, nelem: i32) -> i32;
}

/// The one-minute load average, `None` when the system does not say.
pub fn load1() -> Option<f64> {
    let mut v = [0.0f64; 3];
    // SAFETY: the buffer holds the three doubles asked for.
    let n = unsafe { getloadavg(v.as_mut_ptr(), 3) };
    (n >= 1).then_some(v[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_is_judged_against_the_floor() {
        assert!(is_clean(37.0, 37.0));
        assert!(is_clean(38.8, 37.0));
        assert!(!is_clean(38.9, 37.0));
        assert!(!is_clean(265.9, 36.8));
    }

    #[test]
    fn load_average_reads() {
        let l = load1().expect("getloadavg answers on darwin and linux");
        assert!(l >= 0.0);
    }
}
