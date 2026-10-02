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

// The ToString family moved to `ssa_lower_call_coercion_str` when this
// file reached the 500-line limit (402-01 style split). Re-exported
// here so every call site keeps naming one coercion entry point.
pub(crate) use crate::ssa_lower_call_coercion_str::{emit_to_string, emit_to_string_inner};

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
        // The arg still evaluates for effect (§7.1.17 evaluates its
        // operand): `String(v("x"))` with a void `v` must run the
        // call — the fold only replaces the RESULT. An
        // Undefined-typed value's payload is ConstPtrNull, so there
        // is no owned temp to release. (Pre-fix the fold skipped
        // the lower and the side effect vanished — surfaced by the
        // template-substitution String() wrap.)
        let _ = ctx.lower_expr(args[0]);
        return Some(match n_kind.as_str() {
            "Number" => Operand::ConstF64(f64::NAN),
            "String" => Operand::Value(ctx.intern_string_literal("undefined")),
            "Boolean" => Operand::ConstBool(false),
            _ => unreachable!(),
        });
    }
    // RFC 20260719-fn-tostring-source B6 — `String(Math.max)` (and
    // the template-substitution String() wrap): a namespace-static
    // builtin fn member has no value form to lower; fold the JSC
    // named native form before the operand lower would reject it.
    if n_kind == "String"
        && let Some(text) =
            crate::ssa_lower_call_fn_tostring::namespace_static_native_form(ctx, args[0])
    {
        return Some(Operand::Value(ctx.intern_string_literal(&text)));
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
            let v = ctx.coerce_to_bool(arg_op);
            ctx.release_owned_temp(args[0], &arg_op);
            v
        }
        "String" => emit_to_string(
            ctx,
            args[0],
            arg_op,
            arg_ty,
            ctx.ast.template_str_calls.contains(&callee),
        ),
        _ => unreachable!(),
    })
}

/// Spec §7.1.4 ToNumber dispatch by arg SSA type. Numeric types pass
/// through; Bool → I64; null → 0; Str/Substr → str_to_number (strtod);
/// Any → coerce_any_to_number; Arr → join(",") then str_to_number
/// (Number([1,2]) === NaN).
pub(crate) fn emit_to_number(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_ty: Type,
) -> Operand {
    // A pointer-shaped slot the checker typed `T | undefined` holds
    // the generic immortal cell for its undefined, and the arms
    // below read the slot as a live value of its own type: the Obj
    // arm would hand the cell to OrdinaryToPrimitive, the Arr arm
    // would read a length off it. §7.1.4 step 2 answers NaN for
    // undefined, which is what `Number(q)` on a possibly absent q is
    // asking for.
    //
    // Same pair of predicates, and the same boundary, as the
    // ToString sibling next door: `box_to_any_from_expr` re-encodes
    // the cell as ANY_UNDEF and the §7.1.4 kernel answers NaN, while
    // a live pointer boxes exactly as the heap arm below boxes it.
    // Self-gating on the checker's nullable typing.
    if crate::ssa_lower_nullable_guard::holds_undef_cell(ctx, arg_eid, arg_ty) {
        let boxed = ctx.box_to_any_from_expr(arg_eid, arg_op);
        let n = ctx.coerce_any_to_number(boxed, Type::F64);
        // A user `valueOf` on the live side can throw.
        ctx.emit_throw_check_releasing(arg_eid, &arg_op);
        return n;
    }
    match arg_ty {
        Type::I64 | Type::F64 => arg_op,
        Type::Bool => ctx.coerce_bool_to_i64(arg_op),
        // RFC 20260716 刀 6 — Type::Ptr + ConstPtrNull covers both
        // undefined and null after SSA collapse; ToNumber(undef) is
        // NaN vs ToNumber(null) is 0 per ES §7.1.4. The checker
        // still knows the source frontend type — use it to pick.
        // Mirrors the P1.5 shortcut and 刀 5's binop pack retag.
        Type::Ptr if matches!(arg_op, Operand::ConstPtrNull) => {
            if matches!(
                ctx.expr_types.get(&arg_eid),
                Some(crate::check::Type::Undefined)
            ) {
                Operand::ConstF64(f64::NAN)
            } else {
                Operand::ConstI64(0)
            }
        }
        Type::Str | Type::Substr => {
            // V3-18 m1.h.9 — String → ToNumber via runtime helper
            // (strtod-based, NaN on parse failure). Returns f64 since
            // NaN can't fit i64. The helper borrows; release an owned
            // temp arg after the read.
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_to_number, vec![arg_op]),
                Type::F64,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            Operand::Value(v)
        }
        // §21.1.1.1 step 3 — `Number(bigint)` is the one legal
        // BigInt→Number face: 𝔽(ℝ(value)) via the torajs-bigint
        // kernel. Implicit ToNumber(BigInt) keeps throwing (§7.1.4
        // step 2) — only this explicit-call arm converts.
        Type::BigInt => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.bigint_to_number, vec![arg_op]),
                Type::F64,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            Operand::Value(v)
        }
        // S133-2 — `Number(Any)`: tag-dispatched ToNumber via runtime
        // helper, behind the §21.1.1.1 pre-gate that legally converts
        // a BigInt payload (every other tag answers exactly what the
        // generic kernel answers, Symbol reject included). Returns
        // f64 (NaN passes through). The helper borrows.
        Type::Any => {
            let v = Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.number_ctor_any, vec![arg_op]),
                Type::F64,
                None,
            ));
            // §7.1.4 can record a pending throw (Symbol reject /
            // OrdinaryToPrimitive TypeError) — same check the
            // coerce_any_to_number sink emits.
            ctx.emit_throw_check_releasing(arg_eid, &arg_op);
            v
        }
        Type::Arr(elem_arr_id) => {
            crate::ssa_lower_coerce_arr::emit_to_number(ctx, arg_eid, arg_op, elem_arr_id)
        }
        // §7.1.4 step 8 — ToNumber(object) is ToNumber of what
        // OrdinaryToPrimitive answers with the NUMBER hint, so the
        // receiver's `valueOf` runs and its result is the number. A
        // receiver with no hook lands on Object.prototype's, whose
        // answer is the object itself, so the walk falls to `toString`
        // and NaN. That whole ladder lives in the runtime kernel the
        // any lane already used; the typed spelling boxes the pointer
        // (a pure encode, no rc traffic) and asks the same question.
        // The heap cells reach the same ladder — §7.1.4 step 8 does not
        // care which cell it is, and the kernel reads the header tag:
        // a Date answers its timestamp through `valueOf`, everything
        // else falls to `toString` and NaN. `Number(d as any)` gave 0
        // while `Number(d)` was a compile error.
        Type::Obj(_)
        | Type::RegExp
        | Type::Map
        | Type::Set
        | Type::WeakMap
        | Type::WeakSet
        | Type::WeakRef
        | Type::MapIter
        | Type::ArrIter
        | Type::Date
        | Type::Promise => {
            // A user `valueOf` can throw: the coercion's own check
            // takes the throw edge, so the temp is parked across it.
            let boxed = ctx.box_to_any(arg_op);
            let parked = ctx.park_owned_temp(arg_eid, &arg_op);
            let n = ctx.coerce_any_to_number(boxed, Type::F64);
            ctx.unpark_owned_temp(parked);
            ctx.release_owned_temp(arg_eid, &arg_op);
            n
        }
        // §7.1.4 steps 2-3 — ToNumber(undefined) is NaN, ToNumber(null)
        // is +0. Both are Ptr-shaped once bound to a name, so
        // `Number(null)` answered 0 while `const s = null;
        // Number(s)` panicked here. The checker's record of the type
        // tells the two apart, exactly as the ToString sibling reads
        // it.
        Type::Ptr => {
            if matches!(
                ctx.expr_types.get(&arg_eid),
                Some(crate::check::Type::Undefined)
            ) {
                Operand::ConstF64(f64::NAN)
            } else {
                Operand::ConstF64(0.0)
            }
        }
        _ => panic!("ssa-lower: Number() with arg type {arg_ty:?} not yet supported"),
    }
}
