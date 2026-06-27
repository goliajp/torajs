//! `Expr::New { class_name, args }` builtin-class lowering pulled
//! out of [`crate::ssa_lower::lower_expr_inner`]'s match arms as
//! chunk-78 of the decomp (chunks 1-77 = ... + `Expr::Ident`
//! 6-layer fallback).
//!
//! Six builtin classes covered (all spec-mandated ctor names):
//!
//! - **WeakRef** — `__torajs_weakref_create(target?)`. 0-arg form
//!   passes `ConstPtrNull`; 1-arg form lowers the target.
//! - **WeakMap** — `__torajs_weakmap_create()` (0-arg only).
//! - **WeakSet** — `__torajs_weakset_create()` (0-arg only).
//! - **Map** — `__torajs_map_create()` baseline, then 3 init paths:
//!     - **S133-5**: static `new Map([[k, v], ...])` literal-pair-
//!       array → one `map_set(map, k_tag, k_val, v_tag, v_val)`
//!       per pair. Tightened (S156): only fire when every elem is
//!       a 2-element non-spread pair literal; otherwise fall
//!       through to Arr-fallback (silent empty map elimination).
//!     - **S166**: `new Map(<Map>)` copy-ctor via
//!       `__torajs_map_clone(src)`. Drop the unused baseline Map.
//!     - **S156**: `new Map(<typed Array<Array<T>>>)` — walk the
//!       outer Array via header/body/after blocks; per outer
//!       slot Load inner Array<T>, then index 0/1 as key/value,
//!       `box_to_tag_value` encode, `map_set` write. Drop outer
//!       array post-loop (we own the src).
//! - **Set** — `__torajs_set_create()` baseline + parallel init
//!   paths:
//!     - **S133-4**: static `new Set([a, b, c])` array of
//!       non-Spread elements → per-elem `map_set(set, k_tag,
//!       k_val, ANY_UNDEF=5, 0)`.
//!     - **S166**: `new Set(<Set>)` copy-ctor via
//!       `__torajs_set_union(src, NULL)` (set_union walks src
//!       then NULL; result is clone).
//!     - **S152**: `new Set(<typed Array<T>>)` — walk array,
//!       `box_to_tag_value` per elem, `map_set` write. Drop src
//!       post-loop.
//! - **Array(n)** 1-arg numeric form — `__torajs_arr_alloc_any_filled(n)`
//!   via fn_table lookup (intrinsic not in Intrinsics struct).
//!   Allocates `Array<Any>` of length n with ANY_NULL slots.
//!   0-arg + ≥2-arg forms are rewritten to array literals by
//!   `desugar_builtin_new` and never reach here.
//! - **RegExp** — `__torajs_regex_compile(pattern, flags?)`.
//!   Missing flags arg interns `""` string. `new RegExp(...)`
//!   keeps per-call fresh-alloc semantics (no fn-scope LICM
//!   like `Expr::Regex` literal form gets).
//!
//! Returns `Some(op)` on hit; `None` when `class_name` not in
//! the 6-class allowlist (caller panics).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    class_name: &str,
    args: &[ExprId],
) -> Option<Operand> {
    match class_name {
        "WeakRef" => Some(lower_weakref(ctx, args)),
        "WeakMap" => Some(lower_simple_create(
            ctx,
            ctx.intrinsics.weakmap_create,
            Type::WeakMap,
        )),
        "WeakSet" => Some(lower_simple_create(
            ctx,
            ctx.intrinsics.weakset_create,
            Type::WeakSet,
        )),
        "Map" => Some(lower_map(ctx, args)),
        "Set" => Some(lower_set(ctx, args)),
        "Array" if args.len() == 1 => Some(lower_array_n(ctx, args)),
        "RegExp" => Some(lower_regexp(ctx, args)),
        _ => None,
    }
}

fn lower_weakref(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let target_op = if args.is_empty() {
        Operand::ConstPtrNull
    } else {
        ctx.lower_expr(args[0])
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.weakref_create, vec![target_op]),
        Type::WeakRef,
        None,
    );
    Operand::Value(v)
}

fn lower_simple_create(
    ctx: &mut LowerCtx<'_>,
    intrinsic: crate::ssa::FuncId,
    result_ty: Type,
) -> Operand {
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(intrinsic, vec![]),
        result_ty,
        None,
    );
    Operand::Value(v)
}

fn lower_map(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.map_create, vec![]),
        Type::Map,
        None,
    );
    let map_op = Operand::Value(v);
    if try_map_static_pair_init(ctx, &map_op, args) {
        return map_op;
    }
    let Some(arg0) = args.first() else {
        return map_op;
    };
    let arg_op = ctx.lower_expr(*arg0);
    let arg_ty = ctx.operand_ty(&arg_op);
    if arg_ty == Type::Map {
        return lower_map_clone(ctx, map_op, arg_op);
    }
    if let Type::Arr(outer_id) = arg_ty
        && let Type::Arr(inner_id) = ctx.arr_layouts[outer_id.0 as usize]
    {
        let _ = outer_id;
        return crate::ssa_lower_new_arr_init::lower_map_from_arr(
            ctx, map_op, arg_op, arg_ty, inner_id,
        );
    }
    map_op
}

fn try_map_static_pair_init(ctx: &mut LowerCtx<'_>, map_op: &Operand, args: &[ExprId]) -> bool {
    let Some(arg0) = args.first() else {
        return false;
    };
    let Expr::Array(elems) = ctx.ast.get_expr(*arg0).clone() else {
        return false;
    };
    let all_pairs = elems.iter().all(|e| {
        if matches!(ctx.ast.get_expr(*e), Expr::Spread { .. }) {
            return false;
        }
        if let Expr::Array(pair) = ctx.ast.get_expr(*e) {
            pair.len() == 2
                && pair
                    .iter()
                    .all(|p| !matches!(ctx.ast.get_expr(*p), Expr::Spread { .. }))
        } else {
            false
        }
    });
    if !all_pairs {
        return false;
    }
    for pair_eid in &elems {
        if let Expr::Array(pair) = ctx.ast.get_expr(*pair_eid).clone()
            && pair.len() == 2
        {
            let (k_tag, k_val) = ctx.lower_to_tag_value(pair[0]);
            let (v_tag, v_val) = ctx.lower_to_tag_value(pair[1]);
            let cur_block = ctx.cur_block;
            ctx.f.append_void(
                cur_block,
                InstKind::Call(
                    ctx.intrinsics.map_set,
                    vec![map_op.clone(), k_tag, k_val, v_tag, v_val],
                ),
            );
        }
    }
    true
}

fn lower_map_clone(ctx: &mut LowerCtx<'_>, map_op: Operand, arg_op: Operand) -> Operand {
    ctx.emit_drop_value(map_op, Type::Map);
    let cur_block = ctx.cur_block;
    let cloned = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.map_clone, vec![arg_op.clone()]),
        Type::Map,
        None,
    );
    ctx.emit_drop_value(arg_op, Type::Map);
    Operand::Value(cloned)
}

fn lower_set(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.set_create, vec![]),
        Type::Set,
        None,
    );
    let set_op = Operand::Value(v);
    if try_set_static_init(ctx, &set_op, args) {
        return set_op;
    }
    let Some(arg0) = args.first() else {
        return set_op;
    };
    let arg_op = ctx.lower_expr(*arg0);
    let arg_ty = ctx.operand_ty(&arg_op);
    if arg_ty == Type::Set {
        return lower_set_clone(ctx, set_op, arg_op);
    }
    if let Type::Arr(arr_id) = arg_ty {
        return crate::ssa_lower_new_arr_init::lower_set_from_arr(
            ctx, set_op, arg_op, arg_ty, arr_id,
        );
    }
    set_op
}

fn try_set_static_init(ctx: &mut LowerCtx<'_>, set_op: &Operand, args: &[ExprId]) -> bool {
    let Some(arg0) = args.first() else {
        return false;
    };
    let Expr::Array(elems) = ctx.ast.get_expr(*arg0).clone() else {
        return false;
    };
    let no_spread = elems
        .iter()
        .all(|e| !matches!(ctx.ast.get_expr(*e), Expr::Spread { .. }));
    if !no_spread {
        return false;
    }
    for elem in &elems {
        let (k_tag, k_val) = ctx.lower_to_tag_value(*elem);
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.map_set,
                vec![
                    set_op.clone(),
                    k_tag,
                    k_val,
                    Operand::ConstI64(5),
                    Operand::ConstI64(0),
                ],
            ),
        );
    }
    true
}

fn lower_set_clone(ctx: &mut LowerCtx<'_>, set_op: Operand, arg_op: Operand) -> Operand {
    ctx.emit_drop_value(set_op, Type::Set);
    let cur_block = ctx.cur_block;
    let cloned = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.set_union,
            vec![arg_op.clone(), Operand::ConstPtrNull],
        ),
        Type::Set,
        None,
    );
    ctx.emit_drop_value(arg_op, Type::Set);
    Operand::Value(cloned)
}

fn lower_array_n(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let arg_val = ctx.lower_expr(args[0]);
    let arg_i64 = ctx.coerce_to_i64(arg_val);
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let fid = *ctx
        .fn_table
        .get("__torajs_arr_alloc_any_filled")
        .expect("__torajs_arr_alloc_any_filled intrinsic missing");
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arg_i64]),
        Type::Arr(arr_id),
        None,
    );
    Operand::Value(v)
}

fn lower_regexp(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let pat_op = ctx.lower_expr(args[0]);
    let flag_op = if args.len() == 2 {
        ctx.lower_expr(args[1])
    } else {
        let flag_v = ctx.intern_string_literal("");
        Operand::Value(flag_v)
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.regex_compile, vec![pat_op, flag_op]),
        Type::RegExp,
        None,
    );
    Operand::Value(v)
}
