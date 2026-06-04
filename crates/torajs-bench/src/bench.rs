use core::time::Duration;
use std::time::Instant;

use crate::bencher::Bencher;
use crate::report::report;

/// Target wall-clock per sample (50 ms saturates timer granularity
/// without blowing total bench budget on a 30-sample suite).
const SAMPLE_TARGET_NS: u64 = 50_000_000;

/// Number of samples to aim for before reporting.
const TARGET_SAMPLES: usize = 30;

/// Hard upper bound on total wall-clock per `bench_function` call.
/// At 50 ms × 30 samples = 1.5 s of useful work; 3 s leaves headroom
/// for slow benches without making a bench suite take forever.
const TIME_BUDGET: Duration = Duration::from_secs(3);

/// Iter count used during the cost-estimation probe. Single iter is
/// fine — for fast benches the probe is a few ns; for slow benches
/// (≥ 50 ms per op) we'd land at iters_per_sample = 1 anyway.
const PROBE_ITERS: u64 = 1;

/// Bench session. Drop-in for `criterion::Criterion`. The active
/// configuration is fixed (no per-Bench knob exposed yet); fields
/// reserved for future per-session overrides.
#[derive(Default)]
pub struct Bench {
    _private: (),
}

impl Bench {
    pub fn new() -> Self {
        Self::default()
    }

    /// Time a single bench function and print its result line.
    ///
    /// `name` appears in the report; conventionally `crate_op-scale`
    /// (`acquire_release_hot`, `bigint_add-i64-10k`).
    ///
    /// The closure is called once with a probe `Bencher` to estimate
    /// per-iter cost, then re-called once per sample with a fresh
    /// `Bencher` whose `iters` is sized to land each sample at
    /// `SAMPLE_TARGET_NS`.
    pub fn bench_function<F>(&mut self, name: &str, mut body: F) -> &mut Self
    where
        F: FnMut(&mut Bencher),
    {
        let mut probe = Bencher::new(PROBE_ITERS);
        body(&mut probe);
        let single_ns = probe.elapsed.as_nanos().max(1) as u64;
        let iters_per_sample = (SAMPLE_TARGET_NS / single_ns).max(1);

        let mut samples_ns_per_iter: Vec<f64> = Vec::with_capacity(TARGET_SAMPLES + 4);
        let bench_start = Instant::now();
        while samples_ns_per_iter.len() < TARGET_SAMPLES && bench_start.elapsed() < TIME_BUDGET {
            let mut b = Bencher::new(iters_per_sample);
            body(&mut b);
            let per_iter = b.elapsed.as_nanos() as f64 / iters_per_sample as f64;
            samples_ns_per_iter.push(per_iter);
        }

        report(name, iters_per_sample, &samples_ns_per_iter);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_function_runs_closure_multiple_times() {
        // Body that just adds; counts how many distinct Bencher invocations
        // happened (probe + N samples).
        let mut invocations = 0u64;
        let mut b = Bench::new();
        b.bench_function("test_op", |bencher| {
            invocations += 1;
            bencher.iter(|| 1u64.wrapping_add(2));
        });
        // probe + at least one sample; in practice 30 samples will land
        // before the 3 s budget for a trivial body. Don't assert exact
        // count — the algorithm is allowed to take fewer samples if the
        // body is so slow that the time budget hits first.
        assert!(invocations >= 2);
    }
}
