//! `Object.entries(obj)` namespace static method pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-12
//! of the `Expr::Call` god-arm decomp (chunks 1-11 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter + Arr.flatMap).
//!
//! Four receiver type routes:
//! - `Type::Arr(_)` — `__torajs_arr_entries_by_tag` runtime helper. Picks
//!   the per-element NaN-box tag (1=Bool / 2=I64 / 3=F64 / 4=ANY_HEAP)
//!   from the typed Arr's element layout so the helper stays element-type
//!   agnostic. Returns `Arr<Arr<Any, 2>>` of `[idx_str, val]` pairs.
//! - `Type::Str` — `__torajs_str_entries` runtime helper reading u32
//!   length at `STR_LEN_OFF=8` and looping `__torajs_str_at` to mint
//!   fresh Strs per code unit (same materialize choice as W-O-2 /
//!   W-M-rest). Returns `Arr<Arr<2>>` of `[idx_str, char_str]` pairs.
//! - `Type::Any` (W-J Phase C3) — `__torajs_anyv_own_entries` runtime
//!   helper; struct identity is only known at runtime, so the helper
//!   walks `struct_enum` building an `Arr<Arr<Any>>` of `[name, value]`
//!   pairs. Non-struct cells throw loudly — propagated via
//!   `emit_throw_check`.
//! - `Type::Obj(struct)` — compile-time unfold using `struct_layouts`.
//!   Emit one inner `Arr<Any>` per field with two pushes (key Str +
//!   value tagged-by-type), then store each inner ptr into the outer
//!   `Arr<Arr<Any>>`. Mirrors Object.keys's zero-cost reflection but
//!   yields the (key, value) pair shape JS callers expect.
//!
//! S256 + S297 — ES §20.1.2.5 trailing-arg ignore: widens the
//! `args.len() == 1` gate to `>= 1` and lower-and-drops `args[1..]` for
//! spec L-to-R side-effect order (check.rs already typecheck-drops them).
//!
//! Returns `Some(result)` when callee matches `Object.entries` with
//! `args.len() >= 1`; non-Arr/Str/Any/Obj receivers panic at SSA
//! lower-time (preserving the original block's behavior). `None` lets
//! the caller fall through to the next arm.

use crate::ast::{Expr, ExprId};
use crate::ssa::{ArrId, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

/// The receiver is an instance of the Error family — `Error`, a
/// NativeError, or a user class whose `extends` chain reaches one.
///
/// Those instances own `message` / `stack` with [[Enumerable]]: false
/// (§20.5.6.1.1), which the compile-time struct unfold cannot express:
/// it emits every layout field. The runtime own-walk already gets this
/// right for `any` receivers (its FLAG_ERROR filtering is what makes
/// `Object.keys(new Error("m"))` answer 0), so a statically-typed Error
/// receiver boxes and rides it — the Closure-receiver arm's shape.
///
/// The chain walk matters: FLAG_ERROR is inherited at runtime, so a
/// user `class W extends Error` instance filters exactly like `Error`'s
/// own.
pub(crate) fn error_family_receiver(ctx: &LowerCtx<'_>, recv: ExprId) -> bool {
    let Some(crate::check::Type::ClassRef(name)) = ctx.expr_types.get(&recv) else {
        return false;
    };
    crate::ast::class_reaches_error(ctx.ast, name)
}

/// Try to lower an `Object.entries(obj, ...)` call. Returns `Some` when
/// dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "entries" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    // S256 — widen `== 1` → `>= 1` per ES §20.1.2.5 trailing-arg ignore.
    if args.is_empty() {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    // S297 — lower-and-drop trailing args past the 1 useful obj slot per
    // S256 (S272 idiom). check.rs already type_of'd them.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    // Cluster #4 follow-up (rotation 235) — a typed Closure receiver
    // boxes to any and rides the runtime own-entries walk (mirror of
    // the keys / values arms; a plain fn answers []). Borrow-shaped
    // box, RC-NEUTRAL.
    let arg_op = if matches!(ctx.operand_ty(&arg_op), Type::Closure(_))
        || error_family_receiver(ctx, args[0])
    {
        ctx.box_to_any(arg_op)
    } else {
        arg_op
    };
    let arg_ty = ctx.operand_ty(&arg_op);

    // W-O-3 — Array receiver: bun returns Arr<Arr<[idx_str, val], 2>>.
    if let Type::Arr(arr_id) = arg_ty {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        let val_tag: i64 = match elem_ty {
            Type::Bool => 1,
            Type::I64 | Type::I32 => 2,
            Type::F64 => 3,
            t if t.is_refcounted() => 4,
            other => {
                panic!("ssa-lower: Object.entries arr element type {other:?} not yet supported")
            }
        };
        let inner_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let outer_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Arr(inner_arr_id));
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_entries_by_tag,
                vec![arg_op, Operand::ConstI64(val_tag)],
            ),
            Type::Arr(outer_arr_id),
            None,
        );
        return Some(Operand::Value(v));
    }

    // W-O-3-str — String receiver: returns Arr<Arr<2>> of
    // [idx_str, char_str] pairs.
    if matches!(arg_ty, Type::Str) {
        let inner_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let outer_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Arr(inner_arr_id));
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_entries, vec![arg_op]),
            Type::Arr(outer_arr_id),
            None,
        );
        return Some(Operand::Value(v));
    }

    // W-J Phase C3 — `any` receiver: struct identity only known at
    // runtime. Throws loudly on non-struct cells inside the helper.
    if matches!(arg_ty, Type::Any) {
        let inner_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let outer_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Arr(inner_arr_id));
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_own_entries, vec![arg_op]),
            Type::Arr(outer_arr_id),
            None,
        );
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }

    // Compile-time struct unfold — behind the redefined-member gate,
    // since §20.1.2.5 walks EnumerableOwnProperties and a
    // `defineProperty` can move that set after the unfold was decided.
    // Both arms build the same `Arr<Arr<Any>>`, so the join needs no
    // conversion; the walk is the very helper the `any` arm above
    // uses.
    let layout: Vec<(String, Type)> = match arg_ty {
        Type::Obj(sid) => ctx.struct_layouts[sid.0 as usize].clone(),
        other => panic!("ssa-lower: Object.entries requires a struct arg, got {other:?}"),
    };
    let inner_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let outer_ty = Type::Arr(intern_arr_layout(ctx.arr_layouts, Type::Arr(inner_arr_id)));
    let walk_arg = arg_op.clone();
    let walk_ty = outer_ty.clone();
    Some(crate::ssa_lower_struct_exotic_gate::with_exotic_field_gate(
        ctx,
        &arg_op.clone(),
        outer_ty,
        move |ctx| {
            let boxed = ctx.box_to_any(walk_arg);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.anyv_own_entries, vec![boxed]),
                walk_ty,
                None,
            );
            ctx.emit_throw_check(None);
            Operand::Value(v)
        },
        move |ctx| emit_struct_entries_unfold(ctx, arg_op, &layout),
    ))
}

/// Compile-time unfold of `Object.entries(obj)` against a known
/// `struct_layouts` entry. Allocates the outer `Arr<Arr<Any>>` with the
/// field count, pre-sets `len` so direct stores work, then emits one
/// inner `Arr<Any>` per field with `[key_str, val_any]` pushes, storing
/// each inner ptr at the outer's slot offset (no rc_inc — inner has
/// rc=1 from `arr_alloc_any` and outer takes ownership).
fn emit_struct_entries_unfold(
    ctx: &mut LowerCtx<'_>,
    arg_op: Operand,
    layout: &[(String, Type)],
) -> Operand {
    // RFC 20260714-objlit-accessor blade 6 — one entry per own PROPERTY:
    // an accessor's key is the plain name and its value is what the
    // getter answers (the slot walk emitted `["__getter_v", [Function]]`).
    let props = crate::ssa_lower_struct_own_props::own_props(layout, ctx.fn_sigs);
    let n = props.len() as i64;
    let inner_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let outer_arr_id = intern_arr_layout(ctx.arr_layouts, Type::Arr(inner_arr_id));
    let outer = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(n)]),
        Type::Arr(outer_arr_id),
        None,
    );
    // Pre-set len so direct stores at offset 16+i*8 work.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(n), Operand::Value(outer), ARR_LEN_OFF),
    );
    for (idx, prop) in props.iter().enumerate() {
        let inner_after_val = emit_one_pair(ctx, inner_arr_id, &arg_op, prop);
        // Store inner ptr directly into the outer's slot region
        // (regular Array<T> layout, through the data pointer). No
        // rc_inc — inner has rc=1 from arr_alloc_any and outer takes
        // ownership of that ref.
        let data = ctx.emit_arr_data_ptr(Operand::Value(outer));
        let off = (idx as u64) * 8;
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(inner_after_val), data, off),
        );
    }
    Operand::Value(outer)
}

/// Emit one `(key, value)` inner pair as an `Arr<Any>` of cap=2: push
/// `key_str` first (ANY_HEAP tag, rc_inc'd to match T-10.b
/// `push_any` owning-ref contract), then push `value` with its
/// per-type tag (1=Bool / 2=I64 / 3=F64 / 4=ANY_HEAP / 0=Ptr->null).
/// Returns the latest inner Arr ValueId (post `push_any` second call).
fn emit_one_pair(
    ctx: &mut LowerCtx<'_>,
    inner_arr_id: ArrId,
    arg_op: &Operand,
    prop: &crate::ssa_lower_struct_own_props::OwnProp,
) -> ValueId {
    // Inner Array<Any> with cap=2: [key, value].
    let inner = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![Operand::ConstI64(2)]),
        Type::Arr(inner_arr_id),
        None,
    );
    // Push key — Str literal, ANY_HEAP tag (4). An accessor enumerates
    // under its plain property name, never the synthetic slot spelling.
    let key_str = ctx.intern_string_literal(&prop.key);
    // rc_inc on key str so push_any takes an owning ref (matches
    // T-10.b push_any contract).
    ctx.emit_rc_inc(Operand::Value(key_str));
    let inner_after_key = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push_any,
            vec![
                Operand::Value(inner),
                Operand::ConstI64(4), // ANY_HEAP
                Operand::Value(key_str),
            ],
        ),
        Type::Arr(inner_arr_id),
        None,
    );
    // The property's value: a data field is borrowed out of its slot, a
    // getter's result arrives OWNED (so it skips the rc_inc below —
    // `push_any` takes an owning ref either way).
    let (val_op, owned) = crate::ssa_lower_struct_own_props::emit_prop_value(ctx, arg_op, prop);
    let fty = prop.ty();
    // An `Any` value is a NaN-box, but `push_any` takes a (tag, value)
    // pair — split it at runtime (a set-only property's `undefined`
    // lands here, as does an `any`-typed data field).
    if fty == Type::Any {
        let tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![val_op.clone()]),
            Type::I64,
            None,
        );
        let value = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_value, vec![val_op]),
            Type::I64,
            None,
        );
        return ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_push_any,
                vec![
                    Operand::Value(inner_after_key),
                    Operand::Value(tag),
                    Operand::Value(value),
                ],
            ),
            Type::Arr(inner_arr_id),
            None,
        );
    }
    let (tag, push_val): (i64, Operand) = match fty {
        Type::I64 | Type::I32 => (2, val_op),
        Type::F64 => {
            let bits = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BitCastF64ToI64(val_op),
                Type::I64,
                None,
            );
            (3, Operand::Value(bits))
        }
        Type::Bool => {
            let zext = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ZExtBoolToI64(val_op),
                Type::I64,
                None,
            );
            (1, Operand::Value(zext))
        }
        t if t.is_refcounted() => {
            if !owned {
                ctx.emit_rc_inc(val_op.clone());
            }
            (4, val_op)
        }
        Type::Ptr => (0, Operand::ConstI64(0)),
        other => panic!("not yet supported: Object.entries field type {other:?}"),
    };
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_push_any,
            vec![
                Operand::Value(inner_after_key),
                Operand::ConstI64(tag),
                push_val,
            ],
        ),
        Type::Arr(inner_arr_id),
        None,
    )
}
