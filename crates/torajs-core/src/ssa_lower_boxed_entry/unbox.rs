//! Per-argv-slot unbox + the post-call temp releases of the boxed
//! dual-entry adapter — split out of the parent when the S2.36
//! struct-param coercion arm pushed `build_boxed_entry` past the
//! 200-line fn cap with the file itself two lines under 500 (the
//! recorded watch's split). A child module reaches the parent's
//! private items, so `BoxedEntryIntrinsics` stays private with no
//! visibility churn. Bodies verbatim.

use super::BoxedEntryIntrinsics;
use crate::ssa::{self, FuncId, InstKind, Operand, Type};
use crate::ssa_lower_rc_emit::emit_rc_inc_in_fn;

/// S2.36 — the struct-param coercion trio threaded into one adapter
/// build: the kernel + release FuncIds and the per-user-param
/// class_tag (`Some` only for `Type::Obj` params).
pub(super) struct BoxedCoerceIntrinsics<'a> {
    pub(super) arg_to_struct: FuncId,
    /// `__torajs_anyv_struct_fits(v, class_tag) -> i64` — the mono
    /// adapter's struct-slot test ahead of its unboxes
    /// (`build_reroute`).
    pub(super) struct_fits: FuncId,
    pub(super) anyv_rc_dec: FuncId,
    /// S2.39 — `__torajs_anyv_or_default(v, tag, bits)`: the
    /// literal-default substitution kernel.
    pub(super) anyv_or_default: FuncId,
    pub(super) obj_tags: &'a [Option<u32>],
    /// The twin adapter an under-supplied boolean / integer slot or
    /// a non-fitting struct slot hands the call to (`build_reroute`);
    /// `None` = no lossy slot, or no twin to hand it to.
    pub(super) twin_reroute: Option<super::build_reroute::TwinReroute>,
    /// Per user slot, the argument the reroute test loaded and proved
    /// to fit its struct layout (`__torajs_anyv_struct_fits` answered
    /// 1 on the path that reaches the unboxes); `None` for every
    /// other slot. Such a slot passes the cell through as it is.
    pub(super) fit: Vec<Option<ssa::ValueId>>,
}

/// The adapter's per-param unbox walk (module-doc ABI): load each
/// argv slot and convert to the body's static param type, pushing
/// the call operand and recording owned temps for the post-call
/// releases. `user_tys` and `dflt_lits` are the window's own slice;
/// `first` is the user index of its first slot, for the tables
/// indexed by user slot (`coerce.obj_tags`, `coerce.fit`).
#[allow(clippy::too_many_arguments)]
pub(super) fn unbox_args(
    f: &mut ssa::Function,
    module: &mut ssa::Module,
    entry: ssa::BlockId,
    argv: ssa::ValueId,
    first: usize,
    user_tys: &[Type],
    intr: &BoxedEntryIntrinsics,
    anyv_to_str: FuncId,
    coerce: &BoxedCoerceIntrinsics<'_>,
    args: &mut Vec<Operand>,
    str_temps: &mut Vec<ssa::ValueId>,
    obj_temps: &mut Vec<ssa::ValueId>,
    settle_temps: &mut Vec<(ssa::ValueId, ssa::ValueId)>,
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
                    module.strings.push(ssa::StringLiteral::encode_from_wtf8(s));
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
            // A parameter is a slot, not a ToNumber operand: an
            // `undefined` argument (a hole a kernel read, a missing
            // one) keeps the sentinel, which reads as NaN wherever
            // nothing asks and as `undefined` where something does.
            Type::F64 => Operand::Value(f.append_inst(
                entry,
                InstKind::Call(intr.any_to_f64_slot, vec![av]),
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
            // A struct slot the reroute test proved to fit: the cell
            // has the layout or one extending it, so its pointer
            // bits pass through typed, under the same +1 stake the
            // kernel below takes on a pass-through (released after
            // the call like every obj temp). The kernel's is_cell /
            // tag / layout walk would only repeat the test's.
            Type::Obj(_) if coerce.fit[first + i].is_some() => {
                let av = coerce.fit[first + i].expect("checked above");
                let bits = f.append_inst(
                    entry,
                    InstKind::PtrToInt(Operand::Value(av)),
                    Type::I64,
                    None,
                );
                let ptr =
                    f.append_inst(entry, InstKind::IntToPtr(Operand::Value(bits)), *pty, None);
                emit_rc_inc_in_fn(f, entry, Operand::Value(ptr), *pty);
                obj_temps.push(av);
                Operand::Value(ptr)
            }
            // S2.36 — struct params ride the coercion kernel: an
            // inline-objlit argument arrives as a DYNOBJ box (the
            // any-lane packing contract) while the body reads the
            // param through its struct LAYOUT; the kernel
            // materializes the right repr (or stakes a pass-through)
            // and the owned answer releases after the call.
            Type::Obj(_) => {
                let tag = coerce.obj_tags[first + i].expect("obj_tags aligns with user_tys");
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
            // keeps the reference alive across the call). A ShortStr
            // materialization minted by the unbox is only borrowed by
            // the body: the (box, raw) pair records for the
            // post-call settle (546-02 Class B leg).
            _ => {
                let raw = f.append_inst(
                    entry,
                    InstKind::Call(intr.any_unbox_value, vec![av.clone()]),
                    Type::I64,
                    None,
                );
                if let Operand::Value(av_id) = av {
                    settle_temps.push((av_id, raw));
                }
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

/// 546-02 Class B leg — reclaim ShortStr materializations the
/// heap/ptr param legs minted (each `__torajs_anyv_unbox_settle`
/// no-ops unless its box was a ShortStr), after the body call so
/// the borrow stayed live across it.
pub(super) fn drop_settle_temps(
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
