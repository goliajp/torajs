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
            let v = ctx.coerce_to_bool(arg_op.clone());
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
                InstKind::Call(ctx.intrinsics.str_to_number, vec![arg_op.clone()]),
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
                InstKind::Call(ctx.intrinsics.bigint_to_number, vec![arg_op.clone()]),
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
                InstKind::Call(ctx.intrinsics.number_ctor_any, vec![arg_op.clone()]),
                Type::F64,
                None,
            ));
            // §7.1.4 can record a pending throw (Symbol reject /
            // OrdinaryToPrimitive TypeError) — same check the
            // coerce_any_to_number sink emits.
            ctx.emit_throw_check(None);
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
        // §7.1.4 step 8 — ToNumber(object) is ToNumber of what
        // OrdinaryToPrimitive answers with the NUMBER hint, so the
        // receiver's `valueOf` runs and its result is the number. A
        // receiver with no hook lands on Object.prototype's, whose
        // answer is the object itself, so the walk falls to `toString`
        // and NaN. That whole ladder lives in the runtime kernel the
        // any lane already used; the typed spelling boxes the pointer
        // (a pure encode, no rc traffic) and asks the same question.
        Type::Obj(_) => {
            let boxed = ctx.box_to_any(arg_op.clone());
            let n = ctx.coerce_any_to_number(boxed, Type::F64);
            // A user `valueOf` can throw.
            ctx.emit_throw_check(None);
            ctx.release_owned_temp(arg_eid, &arg_op);
            n
        }
        _ => panic!("ssa-lower: Number() with arg type {arg_ty:?} not yet supported"),
    }
}

/// Spec §7.1.17 ToString dispatch by arg SSA type. Str/Substr pass
/// through; I64/F64/Bool → matching `*_to_str` intrinsic; null →
/// null_to_str; Any → coerce_to_str (tag-dispatched); Arr → join(",")
/// (same dispatch as `arr.toString()`); Obj → "[object Object]" per
/// §20.1.4.4 generic Object toString.
///
/// `implicit_tostring` — true for a parser-synthesized template
/// wrapper (§13.2.8.6 substitution: a Symbol throws TypeError);
/// false for the explicit `String(...)` display face (a Symbol
/// answers its SymbolDescriptiveString per §22.1.1 step 1.a). The
/// two only diverge on Symbol.
pub(crate) fn emit_to_string(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_ty: Type,
    implicit_tostring: bool,
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
        Type::F64 => {
            // RFC 20260708-typed-arr-oob-read chunk 3 — an
            // undefined-infected F64 source (number[] OOB read /
            // its aliases) stringifies the sentinel as "undefined",
            // not "NaN" (same branch the concat lane takes; the
            // template String(...) wrap routes substitutions here).
            if crate::ssa_lower_nullable_guard::is_undef_f64_source(ctx, arg_eid) {
                return crate::ssa_lower_binop_inner::add_str::coerce_undefable_f64(ctx, arg_op).0;
            }
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.f64_to_str, vec![arg_op]),
                Type::Str,
                None,
            ))
        }
        Type::Bool => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.bool_to_str, vec![arg_op]),
            Type::Str,
            None,
        )),
        // RFC 20260716 刀 6 — undef vs null distinction (same
        // rationale as emit_to_number above): ToString(undef) is
        // "undefined" per §7.1.17 step 3, ToString(null) is "null"
        // per §7.1.17 step 2.
        Type::Ptr if matches!(arg_op, Operand::ConstPtrNull) => {
            let fid = if matches!(
                ctx.expr_types.get(&arg_eid),
                Some(crate::check::Type::Undefined)
            ) {
                ctx.intrinsics.undefined_to_str
            } else {
                ctx.intrinsics.null_to_str
            };
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(fid, vec![]),
                Type::Str,
                None,
            ))
        }
        // S133-2 — `String(Any)`: tag-dispatched ToString via the
        // display variant (§22.1.1 step 1.a — the explicit String()
        // call answers a Symbol's SymbolDescriptiveString instead of
        // the §7.1.17 implicit-coercion TypeError; every other tag
        // matches anyv_to_str). A parser-synthesized TEMPLATE
        // wrapper (`ast.template_str_calls`) takes the implicit
        // kernel instead — §13.2.8.6 substitution ToString throws on
        // a Symbol; the two kernels agree on every other tag (both
        // hint-string). Borrows the Any box; release an owned temp
        // arg after the read.
        Type::Any => {
            let fid = if implicit_tostring {
                ctx.intrinsics.any_to_str_box
            } else {
                ctx.intrinsics.any_to_display_str
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(fid, vec![arg_op.clone()]),
                Type::Str,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            // Both kernels can record a pending throw: the implicit
            // one on ToString(Symbol) (§13.2.8.6), and BOTH on an
            // OrdinaryToPrimitive receiver whose toString/valueOf
            // each answer objects (§7.1.17 step 3 TypeError) or whose
            // user hook throws — the display variant only differs on
            // the Symbol arm (test262 String S8.12.8_A1 leaked the
            // placeholder without this).
            ctx.emit_throw_check(None);
            Operand::Value(v)
        }
        // rotation 141 — `String(symbol)` typed spelling: §22.1.1
        // step 1.a SymbolDescriptiveString (the same kernel
        // `sym.toString()` rides; the Any lane's display variant
        // answers this shape already — this is its typed twin). A
        // template wrapper over a statically-typed Symbol takes the
        // implicit kernel through a tag-4 box instead — §13.2.8.6
        // ToString(Symbol) throws (the box is a pure encode; the
        // kernel only reads the borrow).
        Type::Symbol => {
            if implicit_tostring {
                let boxed = ctx.box_to_any(arg_op.clone());
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.any_to_str_box, vec![boxed]),
                    Type::Str,
                    None,
                );
                ctx.release_owned_temp(arg_eid, &arg_op);
                ctx.emit_throw_check(None);
                return Operand::Value(v);
            }
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.symbol_to_str, vec![arg_op.clone()]),
                Type::Str,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            Operand::Value(v)
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
        Type::Obj(_) => emit_struct_to_string(ctx, arg_eid, arg_op),
        // RFC 20260719-fn-tostring-source B5 — ToString(fn) is its
        // toString(): the registry erased-source kernel keyed on the
        // raw fn_addr (FnSig slot) or the closure cell (env-first
        // repr). The template-substitution String() wrap rides this.
        Type::FnSig(_) => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.fn_source_str, vec![arg_op]),
            Type::Str,
            None,
        )),
        Type::Closure(_) => {
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.closure_source_str, vec![arg_op.clone()]),
                Type::Str,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            Operand::Value(s)
        }
        _ => panic!("ssa-lower: String() with arg type {arg_ty:?} not yet supported"),
    }
}

/// S137 — `String(struct)` runs OrdinaryToPrimitive at runtime (RFC
/// 20260712-string-proto-cluster chunk C — `any_to_str` dispatches the
/// user hook and accepts any primitive result, undefined included, and
/// answers the §20.1.4.4 "[object Object]" through
/// Object.prototype.toString when the receiver has no hook of its own).
///
/// It used to shortcut a hook-free layout to a static literal, deciding
/// from the layout's FIELDS whether a hook existed. A class instance
/// has the same `Type::Obj` slot and keeps its methods on the
/// prototype, never in the layout — so that test answered no for every
/// class and would have printed "[object Object]" over a user
/// `toString`. Predicting the hook is what made it wrong; the runtime
/// is the one that knows, and it costs a call on a path nobody's hot
/// loop runs.
fn emit_struct_to_string(ctx: &mut LowerCtx<'_>, arg_eid: ExprId, arg_op: Operand) -> Operand {
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
}
