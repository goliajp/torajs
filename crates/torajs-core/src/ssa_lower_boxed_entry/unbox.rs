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
    /// S2.39 — `__torajs_anyv_or_default(v, tag, bits)`: the
    /// literal-default substitution kernel.
    pub(super) anyv_or_default: FuncId,
    pub(super) obj_tags: &'a [Option<u32>],
}

/// The adapter's per-param unbox walk (module-doc ABI): load each
/// argv slot and convert to the body's static param type, pushing
/// the call operand and recording owned temps for the post-call
/// releases.
#[allow(clippy::too_many_arguments)]
pub(super) fn unbox_args(
    f: &mut ssa::Function,
    module: &mut ssa::Module,
    entry: ssa::BlockId,
    argv: ssa::ValueId,
    user_tys: &[Type],
    intr: &BoxedEntryIntrinsics,
    anyv_to_str: FuncId,
    coerce: &BoxedCoerceIntrinsics<'_>,
    args: &mut Vec<Operand>,
    str_temps: &mut Vec<ssa::ValueId>,
    obj_temps: &mut Vec<ssa::ValueId>,
    dflt_lits: &[Option<super::DfltLit>],
) {
    for (i, pty) in user_tys.iter().enumerate() {
        let mut av = Operand::Value(f.append_inst(
            entry,
            InstKind::Load(Type::Any, Operand::Value(argv), (i as u64) * 8),
            Type::Any,
            None,
        ));
        // S2.39 — a qualifying literal default substitutes when the
        // slot arrives undefined (missing args read the invoke
        // buffer's undefined padding, so the kernel's one undefined
        // test covers both the short-argc and the explicit-undefined
        // form — ES §10.2.1.3 treats them alike). The substitution
        // happens at the BOX level through `__torajs_anyv_or_default`
        // (a branchless kernel call — lower-time SSA has no Select;
        // that inst is egraph-internal), so every per-type unbox arm
        // below stays untouched.
        if let Some(lit) = dflt_lits.get(i).and_then(|l| l.as_ref()) {
            // A long-string literal has no compile-time box — mint a
            // static `.rodata` Str global (rc no-op) and pass its
            // pointer bits under the pair protocol's Heap tag, the
            // direct-call terminal's exact shape. Synthesis runs
            // BEFORE any per-fn body lower, so `module.strings.len()`
            // is the final StringId here — no base/offset deferral.
            let (box_tag, box_bits) = match lit {
                super::DfltLit::StrLong(s) => {
                    let sid = ssa::StringId(module.strings.len() as u32);
                    module.strings.push(ssa::StringLiteral::encode_from_str(s));
                    let ptr = f.append_inst(entry, InstKind::StaticStrRef(sid), Type::Str, None);
                    let p = f.append_inst(
                        entry,
                        InstKind::PtrToInt(Operand::Value(ptr)),
                        Type::I64,
                        None,
                    );
                    (4i64, Operand::Value(p))
                }
                lit => lit
                    .box_encoding()
                    .expect("non-StrLong DfltLit always has a compile-time box"),
            };
            av = Operand::Value(f.append_inst(
                entry,
                InstKind::Call(
                    coerce.anyv_or_default,
                    vec![av, Operand::ConstI64(box_tag), box_bits],
                ),
                Type::Any,
                None,
            ));
        }
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
