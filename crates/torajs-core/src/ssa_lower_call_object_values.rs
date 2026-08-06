//! `Object.values(obj)` namespace static method pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-17
//! of the `Expr::Call` god-arm decomp (chunks 1-16 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from +
//! Arr predicate iter + Arr.flatMap + Object.entries + fn-indirect +
//! Number/String/Boolean coercion + universal methods + closure-local).
//!
//! Four receiver type routes (mirror of [`crate::ssa_lower_call_object_entries`]
//! but emitting `Arr<V>` instead of `Arr<[K, V]>` pairs):
//! - `Type::Arr(_)` — fresh shallow clone via `__torajs_arr_slice` + a
//!   per-element rc_inc range when the element type is refcounted. This
//!   reuses the deep-clone pattern used elsewhere for typed-struct Arr-
//!   field copy — production-tested rc balance.
//! - `Type::Str` — per-char Str array (spec §22.1.5.2 + §20.1.2.20) via
//!   `__torajs_str_to_char_arr`, identical to W-M-rest materialize so the
//!   resulting Strs round-trip through console.log / dynobj stores.
//! - `Type::Any` (W-J Phase C2) — `__torajs_anyv_own_values` runtime
//!   helper; struct identity is only known at runtime, so the helper
//!   reads each field slot and boxes it per its metadata `type_tag`
//!   into an `Arr<Any>`. Non-struct cells throw loudly — propagated via
//!   `emit_throw_check`.
//! - `Type::Obj(struct)` — compile-time unfold using `struct_layouts`.
//!   Homogeneous-field fast path emits one `Load(elem_ty, obj, off)` per
//!   field into an `Arr<elem_ty>`. **S132 heterogeneous-via-`as any`
//!   guard**: when not all field types match `layout[0].1`, the fast
//!   path would emit wrong-typed Loads (e.g. reading a Str ptr through
//!   I64 returns the raw VA as a Number). Detect the mismatch +
//!   box_to_any the receiver + route through the W-J Any-arm walker —
//!   the helper per-field decodes via the field_metadata's type_tag.
//!
//! S256 + S258 + S297 — ES §20.1.2.23 trailing-arg ignore: widens the
//! `args.len() == 1` gate to `>= 1` and lower-and-drops `args[1..]` for
//! spec L-to-R side-effect order (check.rs already typecheck-drops them).
//!
//! Returns `Some(result)` when callee matches `Object.values` with a
//! non-empty args list; non-Arr/Str/Any/Obj receivers panic at SSA
//! lower-time (preserving the original block's behavior). `None` lets
//! the caller fall through to the next arm.

use crate::ast::{Expr, ExprId};
use crate::ssa::{ArrId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, intern_arr_layout};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "values" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    // S258 — widen `== 1` → `>= 1` per ES §20.1.2.23 trailing-arg
    // ignore. Lower only args[0]; trailing dropped at lower-time
    // (check.rs S256/S258 extended arm type_of'd them).
    if args.is_empty() {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    // S297 — lower-and-drop trailing args past the 1 useful obj slot
    // per S258 (S272 idiom). check.rs already type_of'd them.
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    // Cluster #4 follow-up (rotation 235) — a typed Closure receiver
    // boxes to any and rides the runtime own-values walk (the
    // `anyv_own_values` TAG_CLOSURE_CELL arm answers the expando
    // props; the §20.2.4 virtual face is non-enumerable so a plain
    // fn answers []). Borrow-shaped box, RC-NEUTRAL — mirror of the
    // keys lane's arm.
    let arg_op = if matches!(ctx.operand_ty(&arg_op), Type::Closure(_)) {
        ctx.box_to_any(arg_op)
    } else {
        arg_op
    };
    let arg_ty = ctx.operand_ty(&arg_op);

    // W-O — Array receiver: bun returns a fresh shallow array of slot
    // values. Reuses the deep-clone pattern (arr_slice + per-element
    // rc_inc for refcounted elem types) that powers typed-struct Arr-
    // field copy — production-tested rc balance.
    if let Type::Arr(arr_id) = arg_ty {
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, arg_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let cloned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_slice,
                vec![arg_op, Operand::ConstI64(0), Operand::Value(len)],
            ),
            Type::Arr(arr_id),
            None,
        );
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let cloned_len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(cloned), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(
                Operand::Value(cloned),
                Operand::ConstI64(0),
                Operand::Value(cloned_len),
            );
        }
        return Some(Operand::Value(cloned));
    }

    // W-O-2 — String receiver: per-char Str array (spec §22.1.5.2 +
    // §20.1.2.20). Loops `__torajs_str_at` to mint one fresh Str per
    // code unit; same materialize path as W-M-rest so the resulting
    // Strs round-trip through console.log / dynobj stores.
    if matches!(arg_ty, Type::Str) {
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Str);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_to_char_arr, vec![arg_op]),
            Type::Arr(arr_id),
            None,
        );
        return Some(Operand::Value(v));
    }

    // W-J Phase C2 — Any receiver: struct identity is only known at
    // runtime. Route through `__torajs_anyv_own_values`, which reads
    // each field slot and boxes it per its metadata type_tag into an
    // `Arr<Any>`. A non-struct cell throws loudly inside the helper —
    // propagate it.
    if matches!(arg_ty, Type::Any) {
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_own_values, vec![arg_op]),
            Type::Arr(arr_id),
            None,
        );
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }

    let Type::Obj(sid) = arg_ty else {
        panic!("ssa-lower: Object.values requires a struct arg, got {arg_ty:?}");
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    // RFC 20260714-objlit-accessor blade 6 — enumerate PROPERTIES, not
    // layout slots: an accessor is one property whose value comes from
    // its getter ([[Get]]), and a get/set pair is not two of them.
    let props = crate::ssa_lower_struct_own_props::own_props(&layout, ctx.fn_sigs);
    let elem_ty = props[0].ty();
    // S132 — heterogeneous-via-`as any` guard. check.rs enforces
    // homogeneous fields when arg type is Struct (5172 reject-loud),
    // but `<typed> as any` makes check see Type::Any (走 Array<Any> arm)
    // while ssa-lower still sees Type::Obj(sid) because `lower_as_cast`
    // is a no-op for refcounted inner types. Falling through the
    // homogeneous fast path (`elem_ty = layout[0].1`) emits a wrong-
    // type Load for every field — `Object.values(mx as any)` where
    // `class Mixed { n: number; s: string }` reads the str ptr through
    // layout[0]=I64 and returns the raw VA as a Number. Detect the
    // mismatch + box the typed operand + route through the W-J walker
    // (the same Any-arm above), which per-field decodes via the
    // field_metadata's type_tag — the correct heterogeneous path.
    if !props.iter().all(|p| p.ty() == elem_ty) {
        let boxed = ctx.box_to_any(arg_op);
        let arr_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_own_values, vec![boxed]),
            Type::Arr(arr_id),
            None,
        );
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }

    let arr_id = intern_arr_layout(ctx.arr_layouts, elem_ty);
    // §20.1.2.22 walks EnumerableOwnProperties, and a `defineProperty`
    // moves that set at run time — so the unfold stands behind the
    // redefined-member gate. Unlike `Object.entries`, the general arm
    // cannot be the any-lane walk: that answers an `Arr<Any>` while
    // this call site's type is `Arr<elem_ty>`, and boxing every typed
    // `Object.values` to make the two agree would tax the path that is
    // fast precisely because it is not boxed. It emits the same unfold
    // instead, asking per member whether it is hidden right now.
    let layout_hidden = layout.clone();
    let arg_hidden = arg_op.clone();
    Some(crate::ssa_lower_struct_exotic_gate::with_exotic_field_gate(
        ctx,
        &arg_op.clone(),
        Type::Arr(arr_id),
        move |ctx| {
            let props = crate::ssa_lower_struct_own_props::own_props(&layout_hidden, ctx.fn_sigs);
            emit_values_unfold(ctx, &arg_hidden, &props, arr_id, true)
        },
        move |ctx| {
            let props = crate::ssa_lower_struct_own_props::own_props(&layout, ctx.fn_sigs);
            emit_values_unfold(ctx, &arg_op, &props, arr_id, false)
        },
    ))
}

/// The static unfold: one array slot per own property, in declaration
/// order.
///
/// `filtered` swaps fixed slots for pushes and asks the runtime, per
/// member, whether a sidecar has hidden it since. The array is still
/// allocated at the full member count — that is a capacity, and the
/// filtered form simply leaves some of it unused.
fn emit_values_unfold(
    ctx: &mut LowerCtx<'_>,
    arg_op: &Operand,
    props: &[crate::ssa_lower_struct_own_props::OwnProp],
    arr_id: ArrId,
    filtered: bool,
) -> Operand {
    let n = props.len() as i64;
    let arr_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc, vec![Operand::ConstI64(n)]),
        Type::Arr(arr_id),
        None,
    );
    if !filtered {
        // Pre-set len so the direct stores below land in the slot
        // region.
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::ConstI64(n), Operand::Value(arr_ptr), ARR_LEN_OFF),
        );
    }
    for (i, prop) in props.iter().enumerate() {
        let skip_blk = if filtered {
            let key = ctx.intern_string_literal(&prop.key);
            let hidden = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.obj_key_is_nonenumerable,
                    vec![arg_op.clone(), Operand::Value(key)],
                ),
                Type::I64,
                None,
            );
            let keep = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Eq, Operand::Value(hidden), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let keep_blk = ctx.f.add_block();
            let next_blk = ctx.f.add_block();
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(keep),
                    then_blk: keep_blk,
                    else_blk: next_blk,
                },
            );
            ctx.cur_block = keep_blk;
            Some(next_blk)
        } else {
            None
        };
        // A getter's result is already owned; the array takes that ref
        // straight over. A borrowed field slot keeps the pre-blade-6
        // shape (the array views the struct's stake). The push and the
        // store take it the same way — neither adds a reference.
        let (v, _owned) = crate::ssa_lower_struct_own_props::emit_prop_value(ctx, arg_op, prop);
        if let Some(next_blk) = skip_blk {
            let raw = ctx.raw_slot_arg(v);
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.arr_push_unchecked,
                    vec![Operand::Value(arr_ptr), raw],
                ),
            );
            let end = ctx.cur_block;
            ctx.f.set_term(end, Terminator::Br(next_blk));
            ctx.cur_block = next_blk;
        } else {
            let data = ctx.emit_arr_data_ptr(Operand::Value(arr_ptr));
            ctx.f
                .append_void(ctx.cur_block, InstKind::Store(v, data, (i as u64) * 8));
        }
    }
    Operand::Value(arr_ptr)
}
