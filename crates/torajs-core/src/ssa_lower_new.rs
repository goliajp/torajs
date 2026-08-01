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
//!   via fn_table lookup (intrinsic not in Intrinsics struct);
//!   F64 operands route to `__torajs_arr_alloc_any_filled_f64` so the
//!   §23.1.2.1 ToUint32(len) != len RangeError sees the raw bits.
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
        // RFC 20260716 刀 2 — `new Number(x)` / `new String(x)`
        // wrapper alloc. 0-arg forms are pre-desugared to primitive
        // literals by `ast_desugar_builtin_new`.
        "Number" if !args.is_empty() => Some(lower_number_wrapper(ctx, args)),
        "String" if !args.is_empty() => Some(lower_string_wrapper(ctx, args)),
        "Boolean" if !args.is_empty() => Some(lower_boolean_wrapper(ctx, args)),
        // RFC 20260730-iterator-global 刀 1 — §27.1.3.1: the Iterator
        // constructor is abstract; direct `new Iterator(...)` throws
        // a TypeError at runtime (args unevaluated is fine — the
        // throw fires before any observable use; recorded boundary
        // for argument side effects).
        "Iterator" => Some(lower_iterator_ctor_throw(ctx)),
        _ => None,
    }
}

/// `new Iterator(...)` — call the abstract-ctor TypeError kernel and
/// propagate the pending throw (same shape as the RegExp
/// compile-or-throw arm below).
fn lower_iterator_ctor_throw(ctx: &mut LowerCtx<'_>) -> Operand {
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.iterator_ctor_throw, Vec::new()),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// RFC 20260716 刀 2 — `new Number(x)` wrapper alloc. Coerces `x` to
/// f64 via the same ToNumber lattice `Number(x)` callable uses
/// (spec §7.1.4), then calls `__torajs_number_wrapper_new(f64) ->
/// *mut u8` and boxes the result as `Type::Any` (ANY_HEAP tag) so
/// consumers see the same operand shape the checker's `Type::Any`
/// return advertises.
fn lower_number_wrapper(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let arg_eid = args[0];
    let arg_op = ctx.lower_expr(arg_eid);
    let arg_ty = ctx.operand_ty(&arg_op);
    let num_op = crate::ssa_lower_call_coercion::emit_to_number(ctx, arg_eid, arg_op, arg_ty);
    let f64_op = ctx.coerce_to_f64(num_op);
    let cur_block = ctx.cur_block;
    let ptr_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.number_wrapper_new, vec![f64_op]),
        Type::Ptr,
        None,
    );
    ctx.box_to_any(Operand::Value(ptr_v))
}

/// RFC 20260716 刀 2c — `new Boolean(x)` wrapper alloc. Coerces `x`
/// through `LowerCtx::coerce_to_bool` (spec §7.1.2 ToBoolean),
/// then calls `__torajs_boolean_wrapper_new(u8)` and boxes as ANY_HEAP.
fn lower_boolean_wrapper(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let arg_eid = args[0];
    let arg_op = ctx.lower_expr(arg_eid);
    let bool_op = ctx.coerce_to_bool(arg_op.clone());
    ctx.release_owned_temp(arg_eid, &arg_op);
    let cur_block = ctx.cur_block;
    let ptr_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.boolean_wrapper_new, vec![bool_op]),
        Type::Ptr,
        None,
    );
    ctx.box_to_any(Operand::Value(ptr_v))
}

/// RFC 20260716 刀 2b — `new String(x)` wrapper alloc. Coerces `x`
/// through the same `emit_to_string` ladder `String(x)` callable
/// uses (spec §7.1.17), then calls
/// `__torajs_string_wrapper_new(cell)`. The intrinsic has **transfer
/// semantics** — the owned `+1` `emit_to_string` produced is
/// consumed by the wrapper (no rc_inc here; no post-call drop
/// needed). Result is Any-boxed as ANY_HEAP.
fn lower_string_wrapper(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let arg_eid = args[0];
    let arg_op = ctx.lower_expr(arg_eid);
    let arg_ty = ctx.operand_ty(&arg_op);
    let str_op =
        crate::ssa_lower_call_coercion::emit_to_string(ctx, arg_eid, arg_op, arg_ty, false);
    let cur_block = ctx.cur_block;
    let ptr_v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.string_wrapper_new, vec![str_op]),
        Type::Ptr,
        None,
    );
    ctx.box_to_any(Operand::Value(ptr_v))
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
    // §23.1.2.1 step 4.b — a Number len with ToUint32(len) != len is
    // a RangeError. F64 operands go to the f64 alloc entry with the
    // fractional/NaN/Infinity bits intact (the i64 coercion folds
    // NaN → 0 / 4.5 → 4 and the check becomes unfireable);
    // integer-provable operands stay on the hot i64 lane.
    let (alloc_name, alloc_arg) = if matches!(ctx.operand_ty(&arg_val), Type::F64) {
        ("__torajs_arr_alloc_any_filled_f64", arg_val)
    } else {
        ("__torajs_arr_alloc_any_filled", ctx.coerce_to_i64(arg_val))
    };
    let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let fid = *ctx
        .fn_table
        .get(alloc_name)
        .expect("arr alloc intrinsic missing");
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![alloc_arg]),
        Type::Arr(arr_id),
        None,
    );
    // RC-4 F5 — the alloc arms a RangeError for lengths outside
    // [0, 2^32-1] and returns NULL; divert before the NULL is used.
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

fn lower_regexp(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let pat_op = ctx.lower_expr(args[0]);
    let pat_ty = ctx.operand_ty(&pat_op);
    let flag_info = if args.len() >= 2 {
        let f = ctx.lower_expr(args[1]);
        let fty = ctx.operand_ty(&f);
        Some((f, fty))
    } else {
        None
    };
    // `new RegExp(pat, flags)` goes through a throw-aware entry:
    // when the parser rejects the pattern (`[` / `(unbalanced` /
    // `\u{ZZZ}` under `u` / etc.), the kernel records a `SyntaxError`
    // on the TLS pending-throw slot before returning the never-match
    // stub. `emit_throw_check_owned` then propagates that pending
    // throw as a catchable JS exception, dropping the stub RegExp on
    // both catch and propagate branches so the ref never leaks.
    // Literal `/pat/flags` in `ssa_lower_lit.rs` intentionally keeps
    // calling plain `regex_compile` (its call is hoisted to
    // `BlockId(0)` for LICM and needs an entry-block-safe throw-check
    // shape — L3b).
    let str_fast = matches!(pat_ty, Type::Str)
        && flag_info
            .as_ref()
            .is_none_or(|(_, t)| matches!(t, Type::Str));
    let v = if str_fast {
        let flag_op = match flag_info {
            Some((f, _)) => f,
            None => Operand::Value(ctx.intern_string_literal("")),
        };
        let cur_block = ctx.cur_block;
        ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.regex_compile_or_throw, vec![pat_op, flag_op]),
            Type::RegExp,
            None,
        )
    } else {
        // §22.2.3.1 runtime-shaped operands (rotation 267; the
        // RegExp call→construct rewrite exposed non-Str patterns —
        // a RegExp-object pattern read as a Str cell SIGSEGV'd):
        // both box to Any and the kernel dispatches per shape (a
        // RegExp pattern copies source/flags, everything else runs
        // ToString; absent flags ride an undefined box).
        let pat_any = ctx.box_to_any_from_expr(args[0], pat_op);
        let flag_any = match flag_info {
            Some((f, _)) => ctx.box_to_any_from_expr(args[1], f),
            None => {
                let u = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.any_box,
                        vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                    ),
                    Type::Any,
                    None,
                );
                Operand::Value(u)
            }
        };
        let cur_block = ctx.cur_block;
        ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.regex_compile_any, vec![pat_any, flag_any]),
            Type::RegExp,
            None,
        )
    };
    ctx.emit_throw_check_owned(None, Operand::Value(v), Type::RegExp);
    Operand::Value(v)
}
