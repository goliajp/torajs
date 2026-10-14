//! The releases after the body call of the temporaries the parameter
//! legs minted. Moved out of [`super`].

use super::super::BoxedEntryIntrinsics;
use crate::ssa::{self, FuncId, InstKind, Operand};

/// 546-02 Class B leg — reclaim ShortStr materializations the
/// heap/ptr param legs minted (each `__torajs_anyv_unbox_settle`
/// no-ops unless its box was a ShortStr), after the body call so
/// the borrow stayed live across it.
pub(in crate::ssa_lower_boxed_entry) fn drop_settle_temps(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    any_unbox_settle: FuncId,
    temps: &[(ssa::ValueId, ssa::ValueId)],
) {
    for &(av, raw) in temps {
        f.append_void(
            entry,
            InstKind::Call(
                any_unbox_settle,
                vec![Operand::Value(av), Operand::Value(raw)],
            ),
        );
    }
}

/// S2.36 — release the coerced struct-param boxes after the body
/// call (`anyv_rc_dec`: stake balanced / fresh cell freed through
/// the universal heap drop; no-op on immediates the kernel passed
/// through).
pub(in crate::ssa_lower_boxed_entry) fn drop_obj_temps(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    anyv_rc_dec: FuncId,
    temps: &[ssa::ValueId],
) {
    for &t in temps {
        f.append_void(entry, InstKind::Call(anyv_rc_dec, vec![Operand::Value(t)]));
    }
}

/// Release the Str-param ToString temps after the body call. A body
/// that returned one of them holds its own reference (owned-return
/// convention), so the release never frees a live result.
pub(in crate::ssa_lower_boxed_entry) fn drop_str_temps(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    temps: &[ssa::ValueId],
) {
    for &t in temps {
        f.append_void(
            entry,
            InstKind::Call(intr.str_drop, vec![Operand::Value(t)]),
        );
    }
}
