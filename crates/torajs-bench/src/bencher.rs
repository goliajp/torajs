use core::time::Duration;
use std::time::Instant;

/// Per-sample driver passed to the closure body of `Bench::bench_function`.
///
/// Mirrors `criterion::Bencher::iter` so existing bench files port over
/// with a single `use` swap. The driver is single-shot per sample —
/// the harness creates a fresh `Bencher` for each sample, sets `iters`,
/// hands it to the closure, and reads `elapsed` once back.
pub struct Bencher {
    pub(crate) iters: u64,
    pub(crate) elapsed: Duration,
}

impl Bencher {
    pub(crate) fn new(iters: u64) -> Self {
        Self {
            iters,
            elapsed: Duration::ZERO,
        }
    }

    /// Run `body` `self.iters` times and record total elapsed time.
    ///
    /// Each invocation is wrapped in `core::hint::black_box` so the
    /// optimizer cannot hoist or DCE the body across iterations.
    pub fn iter<O, F: FnMut() -> O>(&mut self, mut body: F) {
        let start = Instant::now();
        for _ in 0..self.iters {
            core::hint::black_box(body());
        }
        self.elapsed = start.elapsed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_iters_and_zero_elapsed() {
        let b = Bencher::new(100);
        assert_eq!(b.iters, 100);
        assert_eq!(b.elapsed, Duration::ZERO);
    }

    #[test]
    fn iter_runs_body_iters_times() {
        let mut count = 0u64;
        let mut b = Bencher::new(50);
        b.iter(|| {
            count += 1;
        });
        assert_eq!(count, 50);
        // elapsed is non-zero (50 iters even of a no-op take > 0 ns on
        // any real cpu; assert sanity not precision)
        assert!(b.elapsed > Duration::ZERO);
    }

    #[test]
    fn iter_zero_iters_no_op() {
        let mut count = 0u64;
        let mut b = Bencher::new(0);
        b.iter(|| {
            count += 1;
        });
        assert_eq!(count, 0);
    }
}
