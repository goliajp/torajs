//! Typed `number` value lane of `Map<K, number>` — the lowering half
//! of torajs-collections `value_lane`.
//!
//! A `const` binding — fn-local, main-local or a top-level data
//! global — whose initializer is a zero-argument `new Map()` the
//! checker typed `Map<K, number>` owns a fresh, empty Map. The
//! binding marks it (`__torajs_map_mark_num`); from then on the Map
//! refuses a non-number value at every write, so `get` through that
//! binding reads the value as an f64 slot — the number, or the
//! undefined sentinel on a miss — with no tag dispatch at the consumer.
//!
//! Every other receiver keeps the tagged-Any lane: a parameter, a
//! field, a `let` or an alias may hold a Map nobody marked, whose
//! values are whatever was written. A Map whose alias class reaches an
//! `any`-annotated slot (W-ESC, `num_width::escape`) is not marked
//! either: the any side may write what JS allows, and `get` answers
//! what was written, as bun does.

use crate::ast::{Expr, ExprId, Stmt};
use crate::num_width::SlotKey;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS;

/// A lane binding: a fresh Map, marked here, or a `values()` iterator
/// over a lane receiver. Answers whether it is one, for [`register`]
/// once the binding exists.
pub(crate) fn claim_binding(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    init: ExprId,
    mutable: bool,
    init_val: &Operand,
) -> bool {
    if mutable {
        return false;
    }
    if ctx.operand_ty(init_val) == Type::MapIter {
        return values_of_lane(ctx, init);
    }
    let fresh = fresh_num_map(ctx, init)
        && ctx.operand_ty(init_val) == Type::Map
        && !ctx
            .num_f64_slots
            .slot_escapes_any(&ctx.num_width_local_key(name));
    if fresh {
        emit_mark(ctx, *init_val);
    }
    fresh
}

/// `m.values()` on a lane receiver: every step is a number or undefined.
fn values_of_lane(ctx: &LowerCtx<'_>, init: ExprId) -> bool {
    let Expr::Call { callee, .. } = ctx.ast.get_expr(init) else {
        return false;
    };
    matches!(ctx.ast.get_expr(*callee),
        Expr::Member { obj, name } if name == "values" && is_lane_receiver(ctx, *obj))
}

/// The data-global twin of [`claim_binding`]'s Map half: the top-level declaration
/// of `name` stores `val` into its global slot.
pub(crate) fn mark_global(ctx: &mut LowerCtx<'_>, name: &str, val: Operand) {
    if ctx.operand_ty(&val) == Type::Map && global_is_lane(ctx, name) {
        emit_mark(ctx, val);
    }
}

fn emit_mark(ctx: &mut LowerCtx<'_>, map: Operand) {
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.map_mark_num, vec![map]),
    );
}

/// A zero-argument `new Map()` the checker typed `Map<K, number>`.
fn fresh_num_map(ctx: &LowerCtx<'_>, init: ExprId) -> bool {
    matches!(ctx.ast.get_expr(init),
        Expr::New { class_name, args, .. } if class_name == "Map" && args.is_empty())
        && matches!(ctx.expr_types.get(&init),
            Some(crate::check::Type::Map(_, v)) if **v == crate::check::Type::Number)
}

/// The top-level `const` named `name` is a lane binding. Answerable
/// from any function: a global is read wherever the program is, and
/// functions may lower before main.
fn global_is_lane(ctx: &LowerCtx<'_>, name: &str) -> bool {
    ctx.ast.stmts.iter().any(|s| {
        matches!(s, Stmt::LetDecl { name: n, init, mutable: false, .. }
            if n == name && fresh_num_map(ctx, *init))
    }) && !ctx
        .num_f64_slots
        .slot_escapes_any(&SlotKey::Global(name.to_string()))
}

/// Record the binding [`claim_binding`] claimed. Keyed by its slot, so an
/// inner binding of the same name is a different receiver.
pub(crate) fn register(ctx: &mut LowerCtx<'_>, name: &str) {
    if let Some(li) = ctx.locals.get(name) {
        ctx.num_map_slots.insert(li.slot);
    }
}

/// `src` names a lane `values()` iterator in this frame: the for-of
/// over it steps f64s (`__torajs_map_iter_step_f64`).
pub(crate) fn is_lane_iter(ctx: &LowerCtx<'_>, src: ExprId) -> bool {
    match ctx.ast.get_expr(src) {
        Expr::Ident(n) => ctx
            .locals
            .get(n)
            .is_some_and(|li| li.ty == Type::MapIter && ctx.num_map_slots.contains(&li.slot)),
        _ => false,
    }
}

/// `obj` names a lane Map binding in this frame.
pub(crate) fn is_lane_receiver(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    match ctx.ast.get_expr(obj) {
        Expr::Ident(n) => match ctx.locals.get(n) {
            Some(li) => li.ty == Type::Map && ctx.num_map_slots.contains(&li.slot),
            None => ctx.globals.contains_key(n) && global_is_lane(ctx, n),
        },
        _ => false,
    }
}

/// `m.get(k)` on a lane receiver: the value as an f64, the undefined
/// sentinel for a miss (§24.1.3.6 step 5) or an absent key (typed Map
/// keys are never undefined).
pub(crate) fn emit_get(ctx: &mut LowerCtx<'_>, recv_op: Operand, args: &[ExprId]) -> Operand {
    if args.is_empty() {
        return Operand::ConstF64(f64::from_bits(F64_UNDEF_SENTINEL_BITS));
    }
    let call = match crate::ssa_lower_call_map_dispatch::emit_find_index(ctx, recv_op.clone(), args)
    {
        Ok(idx) => InstKind::Call(ctx.intrinsics.map_entry_get_f64, vec![recv_op, idx]),
        Err((k_tag, k_val, k_raw)) => {
            let call = InstKind::Call(ctx.intrinsics.map_get_f64, vec![recv_op, k_tag, k_val]);
            let v = ctx.f.append_inst(ctx.cur_block, call, Type::F64, None);
            // Chunk 566 share — settle an owned-temp key's mint ref.
            ctx.release_owned_temp(args[0], &k_raw);
            return Operand::Value(v);
        }
    };
    Operand::Value(ctx.f.append_inst(ctx.cur_block, call, Type::F64, None))
}
