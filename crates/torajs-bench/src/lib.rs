//! torajs-bench — in-house micro-benchmark harness.
//!
//! Narrow drop-in conceptual replacement for the criterion crate.
//! The surface is intentionally small — every `crates/torajs-*/benches/*.rs`
//! in the workspace uses only `Criterion::bench_function`, `criterion_group!`,
//! `criterion_main!`, and `black_box`. We mirror that surface and nothing
//! else; no throughput, no benchmark groups, no html reports, no custom
//! measurement traits.
//!
//! Usage:
//!
//! ```ignore
//! use torajs_bench::{Bench, black_box, bench_group, bench_main};
//!
//! fn bench_foo(b: &mut Bench) {
//!     b.bench_function("foo", |bencher| {
//!         bencher.iter(|| black_box(1 + 1));
//!     });
//! }
//!
//! bench_group!(benches, bench_foo);
//! bench_main!(benches);
//! ```
//!
//! Algorithm — modeled on what criterion's "quick" mode does, minus the
//! statistical machinery (mac noise floor ±20-40% means confidence
//! intervals add no information):
//!
//! 1. **Probe** — run body once to estimate single-iter cost.
//! 2. **Pick `iters_per_sample`** so each sample is ≈ 50 ms (saturates the
//!    CPU long enough to drown out timer granularity without bloating
//!    total bench wall-clock).
//! 3. **Collect** samples until either 30 reached or 3 s total wall-clock
//!    elapsed (whichever first).
//! 4. **Report** min / median / mean / stddev to stdout, one line per
//!    bench function.
//!
//! Output format is human-readable single-line per bench; intentionally
//! parseable by `bench/harness` should we ever wire it in.

mod bench;
mod bencher;
mod report;
mod stats;

pub use bench::Bench;
pub use bencher::Bencher;
pub use std::hint::black_box;

/// Declare a bench group function. Emits `pub fn $name()` that, when
/// called, creates a `Bench` and runs every listed bench function
/// against it.
///
/// ```ignore
/// bench_group!(benches, bench_foo, bench_bar);
/// ```
#[macro_export]
macro_rules! bench_group {
    ($name:ident, $($func:path),+ $(,)?) => {
        pub fn $name() {
            let mut bench = $crate::Bench::new();
            $($func(&mut bench);)+
        }
    };
}

/// Emit the bench binary's `main` entry point. Calls each listed group
/// function in declaration order. Pair with `[[bench]] harness = false`
/// in the crate's Cargo.toml.
///
/// ```ignore
/// bench_main!(benches);
/// ```
#[macro_export]
macro_rules! bench_main {
    ($($group:path),+ $(,)?) => {
        fn main() {
            $($group();)+
        }
    };
}
