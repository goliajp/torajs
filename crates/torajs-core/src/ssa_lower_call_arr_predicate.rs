//! `<Array<T>>.{findIndex|findLastIndex|find|findLast|some|every}` short-
//! circuit predicate iteration pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-10
//! of the `Expr::Call` god-arm decomp (chunks 1-9 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from).
//!
//! Six methods share the same per-element predicate-iter shape:
//! - `findIndex` / `findLastIndex` — return i64 index (-1 default)
//! - `find` / `findLast` — return elem_ty (zero/null default)
//! - `some` — return bool (false default, true on first match)
//! - `every` — return bool (true default, false on first miss)
//!
//! `findLastIndex` / `findLast` walk back-to-front (`i = len-1`, step `-1`,
//! cmp `i >= 0`); the rest walk front-to-back. All exit on first match
//! (find* / some) or first miss (every) per ES §23.1.3.{5-10}.
//!
//! Returns `Some(result)` when callee matches `Expr::Member` + one of the
//! 6 method names — even when the receiver is non-array, the original
//! block panicked at SSA lower-time and that behavior is preserved.
//! `None` lets the caller fall through to the next M2/M3/M4 arm below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

mod iter;
use iter::emit_predicate_iter;

/// Try to lower a `xs.<predicate-method>(p, ...)` call. Returns `Some` when
/// dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if !matches!(
        name.as_str(),
        "findIndex" | "findLastIndex" | "find" | "findLast" | "some" | "every"
    ) {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    let recv_ty = ctx.operand_ty(&recv_op);
    if !matches!(recv_ty, Type::Arr(_)) {
        panic!("ssa-lower: `.{name}(...)` on non-array receiver type {recv_ty:?}");
    }
    let elem_ty = ctx.arr_layouts[match recv_ty {
        Type::Arr(id) => id.0 as usize,
        _ => unreachable!(),
    }];
    let src_arr = match recv_op {
        Operand::Value(v) => v,
        _ => unreachable!(),
    };
    let fn_val = ctx.lower_expr(args[0]);
    let fn_ty = ctx.operand_ty(&fn_val);
    // Rotation 261 — a promoted fn-expr predicate (inline Closure in
    // `fnexpr_recv_fns`, or a knife-2 variable-routed binding in
    // `fnexpr_recv_locals`) takes the §23.1.3 thisArg (T) as its
    // leading `__this` arg: args[1] lowers into an Any box
    // (undefined when absent) — the arr_ho knife-4 protocol mirrored
    // onto the predicate family. The box borrows the temp's stake;
    // the release rides the after-loop settlement below.
    let promoted = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[0]),
            Expr::Ident(n) if ctx.ast.fnexpr_recv_locals.contains(n));
    let mut this_temp: Option<(ExprId, Operand)> = None;
    let this_arg: Option<Operand> = if promoted {
        if let Some(&t) = args.get(1) {
            let op = ctx.lower_expr(t);
            let boxed = ctx.box_to_any_from_expr(t, op.clone());
            this_temp = Some((t, op));
            Some(boxed)
        } else {
            Some(Operand::Value(
                crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx),
            ))
        }
    } else {
        None
    };
    // S270 — eval-and-drop trailing args (thisArg + any extras) so side-
    // effect expressions fire per ES §23.1.3.X trailing-arg ignore. SSA-
    // emit reads only args[0] (the predicate cb); a promoted
    // predicate's thisArg lowered above.
    for &t in args.iter().skip(if promoted { 2 } else { 1 }) {
        let _ = ctx.lower_expr(t);
    }
    // Rotation 364 — an argv-face predicate (body reads `arguments`
    // values; collector doc at
    // `arguments_object_collect::collect_hof_anon_argv`) must not
    // take the direct call: its reshaped sig leads with the synthetic
    // argv pointer. The loop body routes it through the boxed
    // variadic dispatch instead (`emit_argv_face_call`), packing the
    // full §23.1.3.{5-10,30} «kValue, k, O» triple.
    let argv_face = matches!(ctx.ast.get_expr(args[0]),
        Expr::Closure { fn_name, .. } if ctx.ast.closure_argv_fns.contains(fn_name))
        || matches!(ctx.ast.get_expr(args[0]),
            Expr::Ident(n) if ctx.ast.closure_argv_locals.contains(n));
    let (result_ty, result_slot) = setup_result_slot(ctx, &name, elem_ty);
    let out = emit_predicate_iter(
        ctx,
        &name,
        src_arr,
        elem_ty,
        fn_val,
        fn_ty,
        this_arg.as_ref(),
        result_ty,
        result_slot,
        argv_face,
    );
    // RFC 20260705 chunk 552 — release owned-shape temps after the
    // loop consumed them (inline arrow's minted env in the cb slot,
    // Call/New-shaped receiver, the boxed thisArg's payload).
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.release_owned_temp(obj, &recv_op);
    if let Some((t, op)) = this_temp {
        ctx.release_owned_temp(t, &op);
    }
    Some(out)
}

/// Allocate the result slot in the entry block + store the per-method default
/// (so a no-match walk returns the JS spec default without conditional stores
/// in the loop post-section).
///
/// Result slot:
///   findIndex / findLastIndex → Type::I64 (-1 default)
///   some / every             → Type::Bool (false / true)
///   find / findLast          → elem_ty (zero-init default)
/// For find / findLast the not-found return is the zero / null of the
/// element type — no `T | undefined` since tr lacks union types. For
/// refcounted elements the null-pointer sentinel makes a meaningful
/// check (`r === null`); for primitives the user must verify existence
/// via findIndex first.
fn setup_result_slot(ctx: &mut LowerCtx<'_>, method: &str, elem_ty: Type) -> (Type, ValueId) {
    let is_find = matches!(method, "find" | "findLast");
    let result_ty = if is_find {
        crate::ssa_lower_nullable_guard::undefable_read_ty(elem_ty)
    } else if matches!(method, "findIndex" | "findLastIndex") {
        Type::I64
    } else {
        Type::Bool
    };
    let result_slot = ctx.alloca_in_entry(result_ty, Some("__pred_res"));
    let default_op: Operand = match method {
        "findIndex" | "findLastIndex" => Operand::ConstI64(-1),
        "some" => Operand::ConstBool(false),
        "every" => Operand::ConstBool(true),
        "find" | "findLast" => match elem_ty {
            // RFC 20260722 chunk D — an I64 elem slot cannot hold the
            // F64 sentinel; the width analysis seeds F64 on every
            // find/findLast receiver's element class, so this arm is
            // only reachable through un-keyed receivers (opaque
            // aliasing) and keeps the historical zero default there.
            Type::I64 => Operand::ConstI64(0),
            // RFC 20260722 chunk D — a number[] miss answers the
            // undefined-NaN sentinel (was 0.0, a silent-wrong hit
            // value); typeof / eq / print / box consumers route
            // through `is_undef_f64_source`'s find arm.
            Type::F64 => Operand::ConstF64(f64::from_bits(
                crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS,
            )),
            // The promoted bool result spells a miss with the tag
            // every tagged value already uses for `undefined` (was
            // `false`, which is a hit value — silent wrong).
            Type::Bool => {
                let undef = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.any_box,
                        vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                    ),
                    Type::Any,
                    None,
                );
                Operand::Value(undef)
            }
            // RC-1 — an Any slot must default to the boxed `undefined`
            // (ES no-match result); raw null-ptr bits are not a valid
            // NaN-box and render as [unknown-any-tag] downstream.
            Type::Any => {
                let undef = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.any_box,
                        vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                    ),
                    Type::Any,
                    None,
                );
                Operand::Value(undef)
            }
            // RFC 20260722-find-miss-undefined-sentinel chunk A — a
            // string[] miss answers the immortal undefined sentinel
            // (§23.1.3.8 no-match = undefined) instead of NULL (which
            // printed "null"); typeof / eq / .length-guard consumers
            // route through `is_nullable_str_source`'s find arm.
            // Static cell: the result slot's scope drop no-ops.
            Type::Str => ctx
                .str_undef_sentinel_for(Type::Str)
                .expect("Str always has a sentinel mapping"),
            // RFC 20260722 chunk B — a refcounted-pointer elem miss
            // answers the generic immortal undefined cell (was NULL,
            // which boxed to an illegal tag4/0 and printed
            // [unknown-any-tag]); typeof / eq / member-guard / box
            // consumers route through `is_undefable_heap_source`'s
            // find arm. Static cell: the result slot's scope drop
            // no-ops.
            t if t.spells_undef_with_generic_cell() => ctx
                .str_undef_sentinel_for(elem_ty)
                .expect("the generic-cell family always has a sentinel mapping"),
            _ => Operand::ConstPtrNull,
        },
        _ => unreachable!(),
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(default_op, Operand::Value(result_slot), 0),
    );
    (result_ty, result_slot)
}
