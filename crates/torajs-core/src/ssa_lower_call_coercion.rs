//! `Number(x)` / `String(x)` / `Boolean(x)` callable coercion dispatch
//! pulled out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` as
//! chunk-14 of the `Expr::Call` god-arm decomp (chunks 1-13 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect).
//!
//! Routes by `Expr::Ident(n)` with `n in {"Number", "String", "Boolean"}`
//! and emits the spec ToNumber / ToString / ToBoolean primitive coercion
//! per ES §7.1.4 / §7.1.17 / §7.1.2, routed by arg's static SSA type.
//!
//! S307 — args[1..] lowered-and-dropped per §21.1.1 / §22.1.1 /
//! §20.3.1 trailing-arg ignore (S272 idiom; check.rs S251 already
//! typecheck-dropped). args.is_empty() returns the ES-canonical zero
//! per kind (0 / "" / false). The `undefined` bare-Ident shortcut emits
//! the spec constants (NaN / "undefined" / false) before lowering
//! since `undefined`/`null` both collapse to `ConstPtrNull` at the
//! runtime layer.
//!
//! Returns `Some(result)` when `n` matches one of the three; `None`
//! lets the caller fall through to the next arm (e.g. BigInt(x)
//! immediately after).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower a `Number(x)` / `String(x)` / `Boolean(x)` callable
/// coercion. Returns `Some` when dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if !matches!(n.as_str(), "Number" | "String" | "Boolean") {
        return None;
    }
    let n_kind = n.clone();
    // S307 — lower-and-drop trailing args[1..] per S272 idiom so step()-
    // style side-effect exprs fire per ES trailing-arg ignore.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    if args.is_empty() {
        return Some(match n_kind.as_str() {
            "Number" => Operand::ConstI64(0),
            "String" => Operand::Value(ctx.intern_string_literal("")),
            "Boolean" => Operand::ConstBool(false),
            _ => unreachable!(),
        });
    }
    // V3-18 m1.h.52 — Number(undefined) → NaN; String(undefined) →
    // "undefined"; Boolean(undefined) → false. Detect via the
    // checker's static type before lowering since undefined/null
    // both collapse to ConstPtrNull at runtime (chunk B — the old
    // bare-Ident name test missed `void 0`, replace A1_T7).
    if matches!(
        ctx.expr_types.get(&args[0]),
        Some(crate::check::Type::Undefined)
    ) || matches!(ctx.ast.get_expr(args[0]), Expr::Ident(n) if n == "undefined")
    {
        return Some(match n_kind.as_str() {
            "Number" => Operand::ConstF64(f64::NAN),
            "String" => Operand::Value(ctx.intern_string_literal("undefined")),
            "Boolean" => Operand::ConstBool(false),
            _ => unreachable!(),
        });
    }
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    // RFC 20260705 ledger #3 — every coerce helper below borrows its
    // arg (str_to_number / coerce_any / arr_join read without rc
    // traffic), so an Ident source keeps its stake and its scope drop;
    // owned temps are released after the read. `String(str)` passes
    // the value through and shares instead (see emit_to_string).
    Some(match n_kind.as_str() {
        "Number" => emit_to_number(ctx, args[0], arg_op, arg_ty),
        "Boolean" => {
            let v = ctx.coerce_to_bool(arg_op.clone());
            ctx.release_owned_temp(args[0], &arg_op);
            v
        }
        "String" => emit_to_string(ctx, args[0], arg_op, arg_ty),
        _ => unreachable!(),
    })
}

/// Spec §7.1.4 ToNumber dispatch by arg SSA type. Numeric types pass
/// through; Bool → I64; null → 0; Str/Substr → str_to_number (strtod);
/// Any → coerce_any_to_number; Arr → join(",") then str_to_number
/// (Number([1,2]) === NaN).
fn emit_to_number(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_ty: Type,
) -> Operand {
    match arg_ty {
        Type::I64 | Type::F64 => arg_op,
        Type::Bool => ctx.coerce_bool_to_i64(arg_op),
        Type::Ptr if matches!(arg_op, Operand::ConstPtrNull) => Operand::ConstI64(0),
        Type::Str | Type::Substr => {
            // V3-18 m1.h.9 — String → ToNumber via runtime helper
            // (strtod-based, NaN on parse failure). Returns f64 since
            // NaN can't fit i64. The helper borrows; release an owned
            // temp arg after the read.
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_to_number, vec![arg_op.clone()]),
                Type::F64,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            Operand::Value(v)
        }
        // S133-2 — `Number(Any)`: tag-dispatched ToNumber via runtime
        // helper. Returns f64 (NaN passes through). The helper borrows.
        Type::Any => {
            let v = ctx.coerce_any_to_number(arg_op.clone(), Type::F64);
            ctx.release_owned_temp(arg_eid, &arg_op);
            v
        }
        // S172 — `Number(Array<T>)` per ES §7.1.4 ToNumber(Array) =
        // ToNumber(ToString(Array)) = ToNumber(arr.join(",")). Mirrors
        // String(Arr) join path below; the resulting Str feeds
        // str_to_number (NaN on non-numeric join result).
        Type::Arr(elem_arr_id) => {
            let elem_ty = ctx.arr_layouts[elem_arr_id.0 as usize];
            let join_fid = match elem_ty {
                Type::Substr => ctx.intrinsics.arr_join_substr,
                Type::I64 => ctx.intrinsics.arr_join_i64,
                Type::F64 => ctx.intrinsics.arr_join_f64,
                Type::Bool => ctx.intrinsics.arr_join_bool,
                Type::Any => ctx.intrinsics.arr_join_any,
                _ => ctx.intrinsics.arr_join,
            };
            let sep = ctx.intern_string_literal(",");
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(join_fid, vec![arg_op.clone(), Operand::Value(sep)]),
                Type::Str,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            let n = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_to_number, vec![Operand::Value(s)]),
                Type::F64,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_drop, vec![Operand::Value(s)]),
            );
            Operand::Value(n)
        }
        _ => panic!("ssa-lower: Number() with arg type {arg_ty:?} not yet supported"),
    }
}

/// Spec §7.1.17 ToString dispatch by arg SSA type. Str/Substr pass
/// through; I64/F64/Bool → matching `*_to_str` intrinsic; null →
/// null_to_str; Any → coerce_to_str (tag-dispatched); Arr → join(",")
/// (same dispatch as `arr.toString()`); Obj → "[object Object]" per
/// §20.1.4.4 generic Object toString.
fn emit_to_string(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_ty: Type,
) -> Operand {
    match arg_ty {
        Type::Str | Type::Substr => {
            // Identity pass-through: the result IS the arg value, and
            // the owned-result invariant makes the consumer release it.
            // A borrow-shaped arg (Ident / Member) therefore shares —
            // +1 here so the source binding keeps its own stake; the
            // old consume path stole the source's single stake (UAF
            // once the result's owner dropped it, reuse-window probe).
            // Owned temps (concat results) transfer their fresh ref.
            if !ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_rc_inc(arg_op.clone());
            }
            arg_op
        }
        Type::I64 => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.i64_to_str, vec![arg_op]),
            Type::Str,
            None,
        )),
        Type::F64 => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.f64_to_str, vec![arg_op]),
            Type::Str,
            None,
        )),
        Type::Bool => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.bool_to_str, vec![arg_op]),
            Type::Str,
            None,
        )),
        Type::Ptr if matches!(arg_op, Operand::ConstPtrNull) => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.null_to_str, vec![]),
            Type::Str,
            None,
        )),
        // S133-2 — `String(Any)`: tag-dispatched ToString via runtime
        // helper (reuses the existing `coerce_to_str(_, Type::Any)`
        // path used by console.log multi-arg). Borrows the Any box;
        // release an owned temp arg after the read.
        Type::Any => {
            let v = ctx.coerce_to_str(arg_op.clone(), Type::Any);
            ctx.release_owned_temp(arg_eid, &arg_op);
            v
        }
        // S137 — `String(arr)` per ES §22.1.3.30 ToString of Array =
        // `arr.join(",")`. Element type picks the matching arr_join
        // intrinsic (same dispatch table as `arr.toString()` in
        // ssa_lower_str).
        Type::Arr(elem_arr_id) => {
            let elem_ty = ctx.arr_layouts[elem_arr_id.0 as usize];
            let join_fid = match elem_ty {
                Type::Substr => ctx.intrinsics.arr_join_substr,
                Type::I64 => ctx.intrinsics.arr_join_i64,
                Type::F64 => ctx.intrinsics.arr_join_f64,
                Type::Bool => ctx.intrinsics.arr_join_bool,
                Type::Any => ctx.intrinsics.arr_join_any,
                _ => ctx.intrinsics.arr_join,
            };
            let sep = ctx.intern_string_literal(",");
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(join_fid, vec![arg_op.clone(), Operand::Value(sep)]),
                Type::Str,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            Operand::Value(s)
        }
        // S137 — `String(struct)`: a struct whose layout carries a
        // `toString` / `valueOf` field must run OrdinaryToPrimitive
        // (RFC 20260712-string-proto-cluster chunk C — the runtime
        // any_to_str dispatches the user hook and accepts any
        // primitive result, undefined included). A hook-free layout
        // keeps the static §20.1.4.4 "[object Object]" emit.
        Type::Obj(sid) => {
            let layout = &ctx.struct_layouts[sid.0 as usize];
            let has_hook = layout
                .iter()
                .any(|(n, _)| n == "toString" || n == "valueOf");
            if has_hook {
                let raw = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::PtrToInt(arg_op.clone()),
                    Type::I64,
                    None,
                );
                let s = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.any_to_str,
                        vec![Operand::ConstI64(4), Operand::Value(raw)],
                    ),
                    Type::Str,
                    None,
                );
                ctx.emit_throw_check(None);
                ctx.release_owned_temp(arg_eid, &arg_op);
                Operand::Value(s)
            } else {
                ctx.release_owned_temp(arg_eid, &arg_op);
                Operand::Value(ctx.intern_string_literal("[object Object]"))
            }
        }
        _ => panic!("ssa-lower: String() with arg type {arg_ty:?} not yet supported"),
    }
}
