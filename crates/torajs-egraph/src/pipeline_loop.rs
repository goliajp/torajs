//! The loop-shaped passes `transform_module` runs as groups: array
//! header words out of loops, and loop-invariant constants into
//! preheaders.

use torajs_core::ssa::Module;

use crate::interval::FactCache;
use crate::pass_gates::gated_pass;
use crate::{fconst_hoist, iconst_hoist, len_promote, load_licm};

/// The passes that take array header words out of loops: the length
/// of a loop that stores it into a register, header-field loads of
/// arrays the loop never writes, then the words only the typed store's
/// cold-arm kernels write. Before throw_check_fold (its
/// loop-entry probe would otherwise sit between the preheader and the
/// header).
pub(crate) fn array_header_passes(module: &mut Module, cache: Option<&FactCache>) {
    let names = cache.map(FactCache::names);
    // Array length word into a register across a loop that stores it
    // (`len_promote`): the pop loop's `str len` → next `ldr len` chain.
    // After branch_fold (its folded arms are no longer loop body),
    // before load_licm, which then sees the loop without the stores.
    // `TORAJS_LEN_PROMOTE_OFF=1` skips (bisect gate).
    gated_pass("LEN_PROMOTE", module, |m| {
        len_promote::promote_lengths(m, names)
    });
    // Array header-field loads out of loops that never write that
    // array's header (allocation-site alias slices, `load_licm`).
    // After branch_fold (a folded-away out-of-range arm is no longer
    // loop body), before throw_check_fold (its loop-entry probe would
    // otherwise sit between the preheader and the header).
    // `TORAJS_LOAD_LICM_OFF=1` skips (bisect gate).
    gated_pass("LOAD_LICM", module, |m| {
        load_licm::hoist_header_loads(m, names)
    });
    // Header words a loop reads but only the typed store's cold-arm
    // kernels write: kept in registers and read again after each
    // kernel call (`load_licm::reread`). After load_licm, which takes
    // the loops no call naming the array touches.
    // `TORAJS_LOAD_REREAD_OFF=1` skips (bisect gate).
    gated_pass("LOAD_REREAD", module, |m| {
        load_licm::reread::reread_header_words(m, names)
    });
}

/// The two loop-constant hoists: each distinct in-loop `ConstF64`
/// operand mints one preheader Copy and body uses read the value,
/// killing the per-iteration 3-inst rematerialization + GPR→FPR
/// crossing (mandelbrot decomposition D1); its i64 twin does the same
/// for a compare's non-imm12 constant (rotation 623, the fill-loop
/// bound). After branch_fold (its folds shrink loop bodies), before
/// select_form / cmp_sink (their formed shapes read the already-
/// hoisted operands). `TORAJS_FCONST_HOIST_OFF=1` /
/// `TORAJS_ICONST_HOIST_OFF=1` skip either (bisect gates).
pub(crate) fn hoist_loop_consts(module: &mut Module) {
    gated_pass("FCONST_HOIST", module, fconst_hoist::hoist_fp_consts);
    gated_pass("ICONST_HOIST", module, iconst_hoist::hoist_int_consts);
}
