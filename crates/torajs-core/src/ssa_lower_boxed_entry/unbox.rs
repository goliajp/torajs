//! Per-argv-slot unbox + the post-call temp releases of the boxed
//! dual-entry adapter — split out of the parent when the S2.36
//! struct-param coercion arm pushed `build_boxed_entry` past the
//! 200-line fn cap with the file itself two lines under 500 (the
//! recorded watch's split). A child module reaches the parent's
//! private items, so `BoxedEntryIntrinsics` stays private with no
//! visibility churn. Bodies verbatim.

use super::BoxedEntryIntrinsics;
use crate::ssa::{self, FuncId, InstKind, Operand, Type};

/// S2.36 — the struct-param coercion trio threaded into one adapter
/// build: the kernel + release FuncIds and the per-user-param
/// class_tag (`Some` only for `Type::Obj` params).
pub(super) struct BoxedCoerceIntrinsics<'a> {
    pub(super) arg_to_struct: FuncId,
    pub(super) anyv_rc_dec: FuncId,
    pub(super) obj_tags: &'a [Option<u32>],
}

/// The adapter's per-param unbox walk (module-doc ABI): load each
/// argv slot and convert to the body's static param type, pushing
/// the call operand and recording owned temps for the post-call
/// releases.
#[allow(clippy::too_many_arguments)]
pub(super) fn unbox_args(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    argv: ssa::ValueId,
    user_tys: &[Type],
    intr: &BoxedEntryIntrinsics,
    anyv_to_str: FuncId,
    coerce: &BoxedCoerceIntrinsics<'_>,
    args: &mut Vec<Operand>,
    str_temps: &mut Vec<ssa::ValueId>,
    obj_temps: &mut Vec<ssa::ValueId>,
) {
    for (i, pty) in user_tys.iter().enumerate() {
        let av = Operand::Value(f.append_inst(
            entry,
            InstKind::Load(Type::Any, Operand::Value(argv), (i as u64) * 8),
            Type::Any,
            None,
        ));
        let arg = match pty {
            Type::Any => av,
            Type::F64 => Operand::Value(f.append_inst(
                entry,
                InstKind::Call(intr.any_to_number, vec![av]),
                Type::F64,
                None,
            )),
            // Width-narrowed number params truncate toward zero —
            // the narrowing proof can't see runtime any-calls, so a
            // fractional argument here is the RFC's documented
            // width-vs-any boundary (fixture keeps to integers).
            Type::I64 | Type::I32 => {
                let d = f.append_inst(
                    entry,
                    InstKind::Call(intr.any_to_number, vec![av]),
                    Type::F64,
                    None,
                );
                Operand::Value(f.append_inst(
                    entry,
                    InstKind::FpToSi(Operand::Value(d)),
                    Type::I64,
                    None,
                ))
            }
            Type::Bool => Operand::Value(f.append_inst(
                entry,
                InstKind::Call(intr.any_to_bool, vec![av]),
                Type::Bool,
                None,
            )),
            // Str params go through ToString (owned) — a borrow
            // unbox would leak the ShortStr materialization.
            Type::Str => {
                let s = f.append_inst(
                    entry,
                    InstKind::Call(anyv_to_str, vec![av]),
                    Type::Ptr,
                    None,
                );
                str_temps.push(s);
                Operand::Value(s)
            }
            // S2.36 — struct params ride the coercion kernel: an
            // inline-objlit argument arrives as a DYNOBJ box (the
            // any-lane packing contract) while the body reads the
            // param through its struct LAYOUT; the kernel
            // materializes the right repr (or stakes a pass-through)
            // and the owned answer releases after the call.
            Type::Obj(_) => {
                let tag = coerce.obj_tags[i].expect("obj_tags aligns with user_tys");
                let coerced = f.append_inst(
                    entry,
                    InstKind::Call(
                        coerce.arg_to_struct,
                        vec![av, Operand::ConstI64(tag as i64)],
                    ),
                    Type::Any,
                    None,
                );
                obj_temps.push(coerced);
                let raw = f.append_inst(
                    entry,
                    InstKind::Call(intr.any_unbox_value, vec![Operand::Value(coerced)]),
                    Type::I64,
                    None,
                );
                Operand::Value(f.append_inst(
                    entry,
                    InstKind::IntToPtr(Operand::Value(raw)),
                    Type::Ptr,
                    None,
                ))
            }
            // Other heap-typed / raw-ptr params: the cell's NaN-box
            // encoding is its pointer bits (borrow — the argv slot
            // keeps the reference alive across the call).
            _ => {
                let raw = f.append_inst(
                    entry,
                    InstKind::Call(intr.any_unbox_value, vec![av]),
                    Type::I64,
                    None,
                );
                Operand::Value(f.append_inst(
                    entry,
                    InstKind::IntToPtr(Operand::Value(raw)),
                    Type::Ptr,
                    None,
                ))
            }
        };
        args.push(arg);
    }
}

/// S2.36 — release the coerced struct-param boxes after the body
/// call (`anyv_rc_dec`: stake balanced / fresh cell freed through
/// the universal heap drop; no-op on immediates the kernel passed
/// through).
pub(super) fn drop_obj_temps(
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

pub(super) fn drop_str_temps(
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
