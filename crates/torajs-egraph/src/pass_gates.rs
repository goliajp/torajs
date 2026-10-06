//! The environment gates every module pass of the pipeline runs
//! behind, and the per-pass module dump.

use torajs_core::ssa::Module;

use crate::pass_timer;

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
    if std::env::var(format!("TORAJS_{key}_OFF")).as_deref() == Ok("1") {
        return None;
    }
    let stats = pass_timer::timed(key, || pass(module));
    if std::env::var(format!("TORAJS_{key}_STATS")).as_deref() == Ok("1") {
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
    if std::env::var("TORAJS_SSA_DUMP_AFTER").as_deref() == Ok(key) {
        module.print();
    }
}
