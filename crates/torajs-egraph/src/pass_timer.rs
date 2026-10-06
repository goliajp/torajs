//! Per-pass wall-clock lines for `tr build`, printed to stderr when
//! `TORAJS_TIME_PHASES` is set, one level under the driver's `egraph`
//! phase line. The variable is read once per `transform_module`; with it
//! unset a pass costs one thread-local load and no clock read.

use std::cell::Cell;
use std::time::Instant;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
}

/// Read `TORAJS_TIME_PHASES` for the pipeline about to run on this thread.
pub(crate) fn arm() {
    ENABLED.set(std::env::var_os("TORAJS_TIME_PHASES").is_some());
}

/// Run `f`, printing its wall time under `label` when armed. A pass that
/// runs twice prints two lines with the same label.
pub(crate) fn timed<R>(label: &str, f: impl FnOnce() -> R) -> R {
    if !ENABLED.get() {
        return f();
    }
    let start = Instant::now();
    let r = f();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    eprintln!("phase   {label} {ms:.3}");
    r
}
