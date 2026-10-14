//! Per-argv-slot unbox + the post-call temp releases of the boxed
//! dual-entry adapter — split out of the parent when the S2.36
//! struct-param coercion arm pushed `build_boxed_entry` past the
//! 200-line fn cap with the file itself two lines under 500 (the
//! recorded watch's split). A child module reaches the parent's
//! private items, so `BoxedEntryIntrinsics` stays private with no
//! visibility churn. Bodies verbatim.

use super::BoxedEntryIntrinsics;
use super::fit_tag::SlotTest;
use crate::ssa::{self, FuncId, InstKind, Operand, Type};
use crate::ssa_lower_rc_emit::emit_rc_inc_in_fn;
pub(super) use drop_temps::{drop_obj_temps, drop_settle_temps, drop_str_temps};

mod drop_temps;

/// The runtime kernels every adapter build calls, declared once per
/// module.
#[derive(Clone, Copy)]
pub(super) struct BoxedCoerceIds {
    /// Single-value ToString for a `string` slot; `undefined` keeps the
    /// Str undefined sentinel (a param, not a ToString site).
    pub(super) anyv_to_str: FuncId,
    /// S2.36 — the struct-param coercion kernel.
    pub(super) arg_to_struct: FuncId,
    /// `__torajs_anyv_struct_fits(v, class_tag, kin) -> i64` — the mono
    /// adapter's struct-slot test ahead of its unboxes
    /// (`build_reroute`).
    pub(super) struct_fits: FuncId,
    /// `__torajs_anyv_arr_fits(v, class_tag, depth, kin) -> i64` — the
    /// same test for an array-of-struct slot, over every element.
    pub(super) arr_fits: FuncId,
    /// `__torajs_anyv_str_fits(v) -> i64` — a string slot's test.
    pub(super) str_fits: FuncId,
    /// `__torajs_anyv_arr_cell(v, shape) -> i64` — an array slot's
    /// test when its elements are not struct-shaped.
    pub(super) arr_cell: FuncId,
    /// `__torajs_anyv_rest_fits(argv, from, argc, shape) -> i64` — the
    /// same question of every argument a typed rest collects.
    pub(super) rest_fits: FuncId,
    /// The release of a coerced struct argument.
    pub(super) anyv_rc_dec: FuncId,
    /// 刀 2 (RFC 20260815-fn-value-rest-spread) — the typed-rest
    /// conversion kernel.
    pub(super) arr_any_to_typed: FuncId,
    /// The twin adapter call of a receiver-polymorphic twin.
    pub(super) invoke_recv_first: FuncId,
    /// The object cast of a class-typed parameter
    /// (`build_cast`), and the pending-throw test after it.
    pub(super) cast_obj: FuncId,
    /// The same, for a plan with an optional member among its fields:
    /// declared only in a program that has such a plan.
    pub(super) cast_obj_members: Option<FuncId>,
    pub(super) throw_check: FuncId,
}

pub(super) fn declare_coerce_intrinsics(
    module: &mut ssa::Module,
    fn_table: &mut std::collections::HashMap<String, FuncId>,
) -> BoxedCoerceIds {
    use Type::{Any, I64, Ptr};
    let cast_obj_members = fn_table.get(crate::ssa_lower_intrinsics_any_cast::MEMBERS_CAST);
    let cast_obj_members = cast_obj_members.copied();
    let mut d = |name: &str, params: &[Type], ret: Type| {
        crate::ssa_lower_synthesize_main::declare_intrinsic(module, fn_table, name, params, ret)
    };
    BoxedCoerceIds {
        anyv_to_str: d("__torajs_anyv_to_str_param", &[Any], Ptr),
        arg_to_struct: d("__torajs_anyv_arg_to_struct", &[Any, I64], Any),
        struct_fits: d("__torajs_anyv_struct_fits", &[Any, I64, I64], I64),
        arr_fits: d("__torajs_anyv_arr_fits", &[Any, I64, I64, I64], I64),
        str_fits: d("__torajs_anyv_str_fits", &[Any], I64),
        arr_cell: d("__torajs_anyv_arr_cell", &[Any, I64], I64),
        rest_fits: d("__torajs_anyv_rest_fits", &[Ptr, I64, I64, I64], I64),
        anyv_rc_dec: d("__torajs_anyv_rc_dec", &[Any], Type::Void),
        arr_any_to_typed: d("__torajs_arr_any_to_typed", &[Ptr, I64], Ptr),
        invoke_recv_first: d(
            "__torajs_anyv_invoke_recv_first",
            &[Ptr, I64, Any, Ptr, I64],
            Any,
        ),
        cast_obj: d("__torajs_anyv_cast_obj", &[Any, I64, Ptr, Type::Str], Ptr),
        cast_obj_members,
        throw_check: d("__torajs_throw_check", &[], I64),
    }
}

/// One adapter build's coercion state: the kernels, and per user
/// param the run-time test of its argument.
pub(super) struct BoxedCoerceIntrinsics<'a> {
    pub(super) ids: BoxedCoerceIds,
    /// S2.39 — `__torajs_anyv_or_default(v, tag, bits)`: the
    /// literal-default substitution kernel.
    pub(super) anyv_or_default: FuncId,
    pub(super) slots: &'a [SlotTest],
    /// The last user slot is a rest parameter.
    pub(super) rest: bool,
    /// The twin adapter an under-supplied boolean / integer slot or
    /// a non-fitting slot hands the call to (`build_reroute`); `None`
    /// = no lossy slot, or no twin to hand it to.
    pub(super) twin_reroute: Option<super::build_reroute::TwinReroute>,
    /// Per user slot, the argument the reroute test loaded and proved
    /// to fit its struct layout (`__torajs_anyv_struct_fits` answered
    /// 1 on the path that reaches the unboxes); `None` for every
    /// other slot. Such a slot passes the cell through as it is.
    pub(super) fit: Vec<Option<ssa::ValueId>>,
    /// Per user slot, the cast of a class-typed
    /// parameter (`build_cast`); such a slot that did not fit is cast
    /// rather than coerced.
    pub(super) casts: Vec<Option<super::build_cast::ParamCast>>,
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
            // A width-narrowed number param: with an all-`any` twin
            // the reroute test hands every argument but an int32 to
            // it (`fit_tag::scalar_tags`), so this conversion is
            // exact; without a twin a fractional or non-number
            // argument still truncates toward zero.
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
            // A struct-shaped slot the reroute test proved to fit:
            // the cell has the layout or one extending it (an array,
            // such cells in every element), so its pointer bits pass
            // through typed, under the same +1 stake the kernel below
            // takes on a pass-through (released after the call like
            // every obj temp). The kernel's is_cell / tag / layout
            // walk would only repeat the test's.
            Type::Obj(_) | Type::Arr(_) if coerce.fit[first + i].is_some() => {
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
            // a class-typed parameter that did not fit takes the
            // checked cast (`build_cast`); another struct param rides
            // the S2.36 coercion kernel
            Type::Obj(_) => match &coerce.casts[first + i] {
                Some(cast) => {
                    super::build_cast::emit_param_cast(f, entry, cast, coerce, intr, av, obj_temps)
                }
                None => coerce_struct_arg(f, entry, coerce, intr, first + i, av, obj_temps),
            },
            // Other heap-typed / raw-ptr params: the cell's NaN-box
            // encoding is its pointer bits (borrow — the argv slot
            // keeps the reference alive across the call). A ShortStr
            // materialization minted by the unbox is only borrowed by
            // the body: the (box, raw) pair records for the
            // post-call settle (546-02 Class B leg).
            _ => {
                let raw = f.append_inst(
                    entry,
                    InstKind::Call(intr.any_unbox_value, vec![av]),
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

/// S2.36 — a struct param rides the coercion kernel: an inline-objlit
/// argument arrives as a DYNOBJ box (the any-lane packing contract)
/// while the body reads the param through its struct LAYOUT; the
/// kernel materializes the right repr (or stakes a pass-through) and
/// the owned answer releases after the call. `slot` is the user slot.
fn coerce_struct_arg(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    coerce: &BoxedCoerceIntrinsics<'_>,
    intr: &BoxedEntryIntrinsics,
    slot: usize,
    av: Operand,
    obj_temps: &mut Vec<ssa::ValueId>,
) -> Operand {
    let tag = coerce.slots[slot]
        .fit
        .expect("a struct slot carries its layout test")
        .tag;
    let coerced = f.append_inst(
        entry,
        InstKind::Call(
            coerce.ids.arg_to_struct,
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
