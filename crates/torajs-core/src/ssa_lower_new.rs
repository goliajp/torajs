//! `Expr::New { class_name, args }` builtin-class lowering pulled
//! out of [`crate::ssa_lower::lower_expr_inner`]'s match arms as
//! chunk-78 of the decomp (chunks 1-77 = ... + `Expr::Ident`
//! 6-layer fallback).
//!
//! The per-class dispatch and the non-collection constructors:
//!
//! - **WeakRef** — `__torajs_weakref_create(target?)`. 0-arg form
//!   passes `ConstPtrNull`; 1-arg form lowers the target.
//! - **WeakMap** / **WeakSet** — `__torajs_weak{map,set}_create()`,
//!   then any initializer argument through the shared walk (the
//!   weak pair has no static fast lane).
//! - **Map** / **Set** — the fill algorithms live in the sibling
//!   `ssa_lower_new_collection`, which also owns the shared
//!   iterable walk the weak pair reaches through
//!   `lower_simple_create`.
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

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};
use crate::ssa_lower_new_collection::{lower_iterable_init, lower_map, lower_set};

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
            args,
            torajs_rc::collection_kind::COLLECTION_WEAKMAP,
        )),
        "WeakSet" => Some(lower_simple_create(
            ctx,
            ctx.intrinsics.weakset_create,
            Type::WeakSet,
            args,
            torajs_rc::collection_kind::COLLECTION_WEAKSET,
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
        // RFC 20260823-proxy-substrate 刀 1 — §10.5.14 ProxyCreate.
        // Result types `Type::Any`: a proxy impersonates its target,
        // so no static variant could honor what it answers.
        "Proxy" => Some(lower_proxy(ctx, args)),
        // RFC 20260823-typedarray-substrate 刀 1 — §25.1.4.1. Types
        // `Type::Any`: a first-slab buffer is reached only through
        // the any-lane kernels, and a `Type::` variant would be a
        // performance claim rather than a correctness one.
        "ArrayBuffer" => Some(lower_arraybuffer(ctx, args)),
        // RFC 20260823-typedarray-substrate 刀 2 — the eleven §23.2
        // constructors share one kernel, keyed by the element-kind
        // discriminant the NAME resolves to at compile time.
        n if crate::ssa_lower_call_typedarray::kind_of_name(n).is_some() => {
            let kind = crate::ssa_lower_call_typedarray::kind_of_name(n).unwrap();
            Some(crate::ssa_lower_call_typedarray::lower(ctx, kind, args))
        }
        _ => None,
    }
}

/// `new ArrayBuffer(length [, options])` — §25.1.4.1. Both
/// coercions (`ToIndex(length)` and the `maxByteLength` option read)
/// stay inside the kernel: each can run user code, and a case that
/// counts the order of its own side effects is the only way anyone
/// notices which ran first.
fn lower_arraybuffer(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let ops = lower_borrowed_any_pair(ctx, args);
    let argv: Vec<Operand> = ops.iter().map(|(op, _, _)| op.clone()).collect();
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arraybuffer_create, argv),
        Type::Any,
        None,
    );
    release_borrowed_any_pair(ctx, ops);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// `new Proxy(target, handler)` — §10.5.14. Both arguments lower as
/// `any`; the kernel does the object check and records the TypeError
/// for a rejected one, so the emit is a plain call plus the throw
/// check. Missing arguments pass `undefined`, which the kernel
/// rejects exactly like any other non-object.
fn lower_proxy(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
    let ops = lower_borrowed_any_pair(ctx, args);
    let argv: Vec<Operand> = ops.iter().map(|(op, _, _)| op.clone()).collect();
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.proxy_create, argv),
        Type::Any,
        None,
    );
    release_borrowed_any_pair(ctx, ops);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// The two-slot BORROWED-`any` argument pair three ctors take:
/// `new Proxy(t, h)`, `Proxy.revocable(t, h)`, and
/// `new ArrayBuffer(len, options)`. Each slot answers
/// `(operand, we_boxed, source)` so the caller can release exactly
/// what it made; a missing slot is `undefined`, which every one of
/// those kernels already has an answer for.
///
/// A LITERAL argument takes the dynobj lane, exactly like an
/// `any`-typed parameter's does (`ssa_lower_call_terminal`): the
/// target and the handler are reached ONLY through the any lane, and
/// a nominal struct answers different questions there — its fields
/// are not configurable, so a trap-less `delete p.k` refused on a
/// literal that spells an ordinary object.
pub(crate) fn lower_borrowed_any_pair(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Vec<(Operand, bool, Option<ExprId>)> {
    let mut ops: Vec<(Operand, bool, Option<ExprId>)> = Vec::with_capacity(2);
    for slot in 0..2usize {
        let Some(&eid) = args.get(slot) else {
            let undef = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            ops.push((Operand::Value(undef), false, None));
            continue;
        };
        if matches!(ctx.ast.get_expr(eid), crate::ast::Expr::ObjectLit { .. }) {
            let dynobj = ctx.lower_dynobj_init(eid);
            let boxed = ctx.box_to_any(dynobj);
            ops.push((boxed, true, None));
            continue;
        }
        let raw = ctx.lower_expr(eid);
        let ty = ctx.operand_ty(&raw);
        let (op, we_boxed) = match ty {
            Type::Any => (raw, false),
            Type::Arr(_) => {
                // A proxied array keeps its identity — the cell goes
                // in as-is, kind-marked like every other Arr that
                // crosses into the any world.
                ctx.emit_arr_mark_kind(&raw);
                let as_i64 =
                    ctx.f
                        .append_inst(ctx.cur_block, InstKind::PtrToInt(raw), Type::I64, None);
                let as_any = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::IntToPtr(Operand::Value(as_i64)),
                    Type::Any,
                    None,
                );
                (Operand::Value(as_any), false)
            }
            _ => {
                if !ctx.expr_transfers_ownership(eid) && ty.is_refcounted() {
                    ctx.emit_rc_inc(raw.clone());
                }
                (ctx.box_to_any(raw), true)
            }
        };
        ops.push((op, we_boxed, Some(eid)));
    }
    // Trailing arguments beyond the spec's two — lower for effects.
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    ops
}

/// The kernel borrows both arguments, so whatever
/// [`lower_borrowed_any_pair`] made is released after the call.
pub(crate) fn release_borrowed_any_pair(
    ctx: &mut LowerCtx<'_>,
    ops: Vec<(Operand, bool, Option<ExprId>)>,
) {
    for (op, we_boxed, eid) in ops {
        if we_boxed {
            ctx.emit_drop_value(op, Type::Any);
        } else if let Some(eid) = eid {
            ctx.release_owned_temp(eid, &op);
        }
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
    args: &[ExprId],
    kind: i64,
) -> Operand {
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(intrinsic, vec![]),
        result_ty.clone(),
        None,
    );
    let target = Operand::Value(v);
    // The weak pair has no static fast lane: every initializer shape
    // (including a nullish one) is the general walk.
    let Some(arg0) = args.first() else {
        return target;
    };
    let arg_op = ctx.lower_expr(*arg0);
    let arg_owned = ctx.expr_transfers_ownership(*arg0);
    lower_iterable_init(ctx, target, result_ty, arg_op, arg_owned, kind)
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
