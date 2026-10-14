//! The rewrites that read the interval facts after float_demote.

use crate::id_hash::HashMap;

use torajs_core::ssa::{Module, ValueId};

use crate::{cmp_fact_fold, frem_guard, frem_int, gated_pass, interval, srem_counter, srem_fquot};

/// Rewrites that read the interval facts after float_demote (facts
/// stay id-valid across both).
pub(crate) fn fact_folds(module: &mut Module, facts: &mut [HashMap<ValueId, interval::NumFact>]) {
    // Interval-decided compare absorption — frem_narrow's fused zero
    // guard over a divisor the facts prove non-zero collapses onto the
    // remainder compare. `TORAJS_CMP_FACT_FOLD_OFF=1` skips.
    gated_pass("CMP_FACT_FOLD", module, |m| {
        cmp_fact_fold::fold_decided_cmps(m, facts)
    });
    // Exact float remainder — `frem(sitofp x, C)` over a dividend in
    // [0, 2^53) becomes `sitofp(srem x, C)`, and a concat_f64 reader
    // takes the integer. `TORAJS_FREM_INT_OFF=1` skips.
    gated_pass("FREM_INT", module, |m| frem_int::exact_frems(m, facts));
    // a `frem` over operands that may be integers gets a checked integer arm
    gated_pass("FREM_GUARD", module, |m| frem_guard::guard_frems(m, facts));
    // a loop counter's remainder by a divisor the loop keeps becomes a
    // second counter that wraps; the remainder's facts still hold
    gated_pass("SREM_COUNTER", module, |m| {
        srem_counter::count_srems(m, facts)
    });
    // a divisibility test in a loop takes its quotient from `fdiv`; the
    // rewritten remainders' facts are dropped (their ids now name the
    // product), so sext32_set below reads none of them
    gated_pass("SREM_FQUOT", module, |m| {
        srem_fquot::float_quotient_srems(m, facts)
    });
}
