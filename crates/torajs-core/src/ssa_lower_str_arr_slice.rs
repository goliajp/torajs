//! `<Arr>.slice(start?, end?)` + zero-arg `<Arr>.{indexOf,lastIndexOf,includes}()`
//! short-circuit dispatch — eleventh sub-split carved out of
//! [`ssa_lower_str::try_lower_method_call`].
//!
//! Two adjacent fast-paths live together because they are both
//! Array-receiver dispatches that gate on the same `recv_ty == Arr`
//! check and the slice block precedes the full indexOf/lastIndexOf/
//! includes loop (still in `ssa_lower_str.rs` for now):
//!
//! 1. `arr.slice(start?, end?)` — fresh array of the [start, end)
//!    range, single memcpy via `__torajs_arr_slice`. Phase B refcount
//!    inc per non-Copy element so source and derived can both safely
//!    walk-drop.
//! 2. Zero-arg `arr.indexOf() / .lastIndexOf() / .includes()` —
//!    ES §23.1.3.{14,17,18}: `searchElement` defaults to `undefined`.
//!    For typed `Array<T>` with `T ≠ Any`, undefined cannot strictly
//!    equal any element, so the result is fixed: indexOf/lastIndexOf
//!    → `-1`, includes → `false`. `Array<Any>` is left to the existing
//!    strict-arity reject (L3b — needs an Any-tagged undefined sentinel
//!    needle).
//!
//! Returns `None` when the method is neither `slice` nor one of the
//! zero-arg short-circuit cases (or when the receiver is not an Arr)
//! so the caller can keep trying the remaining branches.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower `<Arr>.slice(start?, end?)` or zero-arg
/// `<Arr>.{indexOf,lastIndexOf,includes}()`. Returns `Some(value)`
/// when handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `arr.slice(start, end)` — fresh array of the
    // [start, end) range, single memcpy via
    // __torajs_arr_slice. Element type carried over
    // from the receiver. Phase B refcount: when the
    // element type is non-Copy (Str etc.), the derived
    // array's slots alias the source's; inc each slot's
    // refcount so the source and derived can both safely
    // walk-drop their elements.
    // S242 — Array<T>.slice(start, end, ...trailing) trailing-arg
    // ignore per ES §23.1.3.28: args[2..] is never read; the end
    // branch below switches on `args.len() >= 2 && !arg1_undef`.
    // S299 — widen upper-cap `args.len() <= 3` → no cap + lower-and-drop
    // args[2..] so step()-style side-effect exprs fire per ES eval-then-
    // discard semantics. Mirrors check.rs S299.
    if let Type::Arr(arr_id) = recv_ty
        && method == "slice"
    {
        // §23.1.3.28 step 3 ArraySpeciesCreate — constructor-face
        // guard before the derive (RFC 20260713 blade 3).
        ctx.emit_arr_species_guard(recv_op.clone());
        for &a in args.iter().skip(2) {
            let _ = ctx.lower_expr(a);
        }
        // V3-18 m1.h.35 — JS spec §22.1.3.25 defaults:
        //   arr.slice()      = arr.slice(0, len)
        //   arr.slice(start) = arr.slice(start, len)
        // Read len once when needed; use it as the
        // default for the missing 2nd arg.
        let mut argv = Vec::with_capacity(3);
        argv.push(recv_op);
        // S213 — typed-Undefined for either operand defaults to
        // the spec value per ES §23.1.3.27 step 1-2: start
        // defaults to 0, end defaults to receiver length.
        let arg0_undef = args
            .first()
            .map(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Undefined)))
            .unwrap_or(false);
        let arg1_undef = args
            .get(1)
            .map(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Undefined)))
            .unwrap_or(false);
        // S334 — `xs.slice(Any [, Any])` per ES §23.1.3.28:
        // ToIntegerOrInfinity accepts arbitrary-typed input. Decode
        // Any via anyv_to_number → coerce_to_i64 so the helper's
        // (Arr, i64, i64) ABI sees clean i64s. Sister to S332/S333.
        let arg0_any = args
            .first()
            .map(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Any)))
            .unwrap_or(false);
        let arg1_any = args
            .get(1)
            .map(|a| matches!(ctx.expr_types.get(a), Some(crate::check::Type::Any)))
            .unwrap_or(false);
        let start = if args.is_empty() || arg0_undef {
            Operand::ConstI64(0)
        } else if arg0_any {
            let raw = ctx.lower_expr(args[0]);
            let f = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                Type::F64,
                None,
            );
            ctx.coerce_to_i64(Operand::Value(f))
        } else {
            ctx.lower_expr(args[0])
        };
        argv.push(start);
        let end = if args.len() >= 2 && !arg1_undef {
            if arg1_any {
                let raw = ctx.lower_expr(args[1]);
                let f = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_to_number, vec![raw]),
                    Type::F64,
                    None,
                );
                ctx.coerce_to_i64(Operand::Value(f))
            } else {
                ctx.lower_expr(args[1])
            }
        } else {
            // Load receiver's len from offset 8.
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            Operand::Value(len)
        };
        argv.push(end);
        // Any elem: route through the FLAG_ARR_ANY-aware slice — the
        // raw arr_slice product is flag-blind (runtime flag-dispatch
        // walkers — cycle collector, value_drop_heap — go blind on
        // it) and the helper incs its NaN-box slots itself, so the
        // raw inc walk below must not run (double-inc).
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        let elem_is_any = matches!(elem_ty, Type::Any);
        let slice_fid = if elem_is_any {
            ctx.intrinsics.arr_any_slice
        } else {
            ctx.intrinsics.arr_slice
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(slice_fid, argv),
            Type::Arr(arr_id),
            None,
        );
        if elem_ty.is_refcounted() && !elem_is_any {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(
                Operand::Value(v),
                elem_ty,
                Operand::ConstI64(0),
                Operand::Value(len),
            );
        }
        return Some(Operand::Value(v));
    }
    // `arr.indexOf(needle)` / `arr.lastIndexOf(needle)` /
    // `arr.includes(needle)` — inline SSA loop. indexOf
    // returns the first match index (-1 on miss);
    // lastIndexOf scans from the end (-1 on miss);
    // includes returns a boolean. All three share the
    // per-element compare dispatch (ICmp / FCmp / str_eq).
    // ES §23.1.3.{14,17,18} — 0-arg `arr.indexOf() / .includes() /
    // .lastIndexOf()`: `searchElement` defaults to `undefined`. For
    // typed Array<T> with T ≠ Any, undefined cannot strictly equal
    // any element, so the result is fixed: indexOf/lastIndexOf → -1,
    // includes → false. Short-circuit before the full inline loop.
    // Array<Any> is left to the existing strict-arity reject (L3b —
    // needs an Any-tagged undefined sentinel needle).
    if let Type::Arr(arr_id) = recv_ty
        && (method == "indexOf" || method == "lastIndexOf" || method == "includes")
        && args.is_empty()
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty != Type::Any {
            if method == "includes" {
                return Some(Operand::ConstBool(false));
            }
            return Some(Operand::ConstI64(-1));
        }
    }
    None
}
