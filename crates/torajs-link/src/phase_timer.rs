//! Wall-clock phase ticks for `tr build`, printed to stderr when
//! `TORAJS_TIME_PHASES` is set. One `Instant::now()` per tick and no
//! output otherwise, so the timer stays in the shipped binary.

use std::time::Instant;

pub struct PhaseTimer {
    enabled: bool,
    last: Instant,
    depth: usize,
}

impl PhaseTimer {
    pub fn start(depth: usize) -> Self {
        PhaseTimer {
            enabled: std::env::var_os("TORAJS_TIME_PHASES").is_some(),
            last: Instant::now(),
            depth,
        }
    }

    /// Print the time since the previous tick under `label` and
    /// restart the clock.
    pub fn tick(&mut self, label: &str) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        let ms = now.duration_since(self.last).as_secs_f64() * 1000.0;
        eprintln!("phase {}{label} {ms:.2}", "  ".repeat(self.depth));
        self.last = now;
    }
}
