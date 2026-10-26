//! The environment gates every module pass of the pipeline runs
//! behind, and the per-pass module dump.

use std::cell::RefCell;

use torajs_core::ssa::Module;

use crate::pass_timer;

thread_local! {
    /// The `TORAJS_*` variables of the environment, read once by
    /// [`arm`] for the pipeline about to run on this thread: a lookup
    /// in the environment costs thousands of instructions, and the
    /// pipeline asks three per pass.
    static GATES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// Read the gate variables for the pipeline about to run on this
/// thread. A variable set after this is not seen until the next call.
pub(crate) fn arm() {
    let vars = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .filter(|(k, _)| k.starts_with("TORAJS_"))
        .collect();
    GATES.set(vars);
}

/// Is `TORAJS_<key><suffix>` set to `want`?
fn gate_is(key: &str, suffix: &str, want: &str) -> bool {
    GATES.with_borrow(|g| {
        g.iter().any(|(k, v)| {
            v == want
                && k.strip_prefix("TORAJS_")
                    .and_then(|k| k.strip_prefix(key))
                    .is_some_and(|k| k == suffix)
        })
    })
}

/// `TORAJS_<key>_OFF=1` is set: for a step another pass runs inside
/// itself rather than as a module pass of its own.
pub(crate) fn gate_off(key: &str) -> bool {
    gate_is(key, "_OFF", "1")
}

/// Run one module pass behind the pair of environment gates every
/// pass in the pipeline carries: `TORAJS_<KEY>_OFF=1` skips it
/// (bisect gate) and `TORAJS_<KEY>_STATS=1` dumps its counters as
/// `torajs-<key>-stats:`. Returns the counters when the pass ran.
///
/// `key` is the screaming-snake pass name (`"BRANCH_FOLD"`); both
/// variable names and the stats label derive from it, so a new pass
/// can't drift into a mismatched gate/label triple.
pub(crate) fn gated_pass<S: std::fmt::Debug>(
    key: &str,
    module: &mut Module,
    pass: impl FnOnce(&mut Module) -> S,
) -> Option<S> {
    if gate_is(key, "_OFF", "1") {
        return None;
    }
    let stats = pass_timer::timed(key, || pass(module));
    if gate_is(key, "_STATS", "1") {
        let label = key.to_lowercase().replace('_', "-");
        eprintln!("torajs-{label}-stats: {stats:?}");
    }
    dump_after(key, module);
    Some(stats)
}

/// `TORAJS_SSA_DUMP_AFTER=<KEY>` — pretty-print the whole module to
/// stdout right after the named pass, `KEY` being the same screaming-
/// snake name its `_OFF` / `_STATS` gates use (plus `EGRAPH` for the
/// per-function egraph loop, which is not a `gated_pass`).
///
/// The end-of-pipeline `TORAJS_SSA_DUMP` answers "what shape did the
/// pipeline produce"; this answers "which pass produced it". Diffing
/// the dump across two runs is how a nondeterministic pass gets named:
/// walk the keys in pipeline order and the first one whose dump varies
/// owns the divergence. Toggling `_OFF` gates cannot do that job —
/// passes feed each other, so several will each "fix" one source.
///
/// A pass gated off never reaches here, so asking to dump after a
/// disabled pass prints nothing.
pub(crate) fn dump_after(key: &str, module: &Module) {
    if gate_is("SSA_DUMP_AFTER", "", key) {
        module.print();
    }
}
