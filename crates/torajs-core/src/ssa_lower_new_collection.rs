//! The collection constructors' fill algorithms — `new Map(…)`,
//! `new Set(…)` and the four-shape iterable walk they share with the
//! weak pair. Split out of `ssa_lower_new` because it is one subject
//! (how a collection constructor gets its entries) rather than one
//! more entry in that file's per-class dispatch.
//!
//! Three lanes, narrowest first: a literal `[[k, v], …]` / `[a, b]`
//! source whose entries the lowering can see; a typed source it knows
//! the shape of; and everything else, which goes to the runtime walk.
//! All three perform §24.1.1.1 step 7.a — the adder is read off the
//! target once — because a source the lowering happens to see through
//! is not a reason for the constructor to skip its own algorithm.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// §24.1.1.1 step 7 and its three siblings — the general iterable
/// initializer, reached when the static lanes above could not see the
/// source's shape. A nullish argument lands here too and adds nothing
/// (step 6), which is why the refusal it replaced was wrong about
/// `new Map(null)` as much as about `new Map(someGenerator())`.
pub(crate) fn lower_iterable_init(
    ctx: &mut LowerCtx<'_>,
    target: Operand,
    target_ty: Type,
    arg_op: Operand,
    arg_owned: bool,
    kind: i64,
) -> Operand {
    let arg_ty = ctx.operand_ty(&arg_op);
    let target_any = ctx.box_to_any(target.clone());
    let src_any = ctx.box_to_any(arg_op.clone());
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.collection_init_from_iterable,
            vec![target_any, src_any, Operand::ConstI64(kind)],
        ),
    );
    ctx.emit_throw_check(None);
    // Rotation 325 — the init consumes an OWNED source temp; an
    // ident-bound borrow keeps its binding's stake (dropping it here
    // stole that stake, and the binding's scope-end release then dec'd
    // through freed entries — the census underflow on the
    // collection-init family).
    if arg_owned {
        ctx.emit_drop_value(arg_op, arg_ty);
    }
    let _ = target_ty;
    target
}

pub(crate) fn lower_map(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
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
    let arg_owned = ctx.expr_transfers_ownership(*arg0);
    let arg_ty = ctx.operand_ty(&arg_op);
    if arg_ty == Type::Map {
        return lower_map_clone(ctx, map_op, arg_op, arg_owned);
    }
    if let Type::Arr(outer_id) = arg_ty
        && let Type::Arr(inner_id) = ctx.arr_layouts[outer_id.0 as usize]
    {
        let _ = outer_id;
        return crate::ssa_lower_new_arr_init::lower_map_from_arr(
            ctx, map_op, arg_op, arg_ty, arg_owned, inner_id,
        );
    }
    lower_iterable_init(
        ctx,
        map_op,
        Type::Map,
        arg_op,
        arg_owned,
        torajs_rc::collection_kind::COLLECTION_MAP,
    )
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
    let adder = lower_static_adder(ctx, map_op, torajs_rc::collection_kind::COLLECTION_MAP);
    for pair_eid in &elems {
        if let Expr::Array(pair) = ctx.ast.get_expr(*pair_eid).clone()
            && pair.len() == 2
        {
            let (k_tag, k_val) = ctx.lower_to_tag_value(pair[0]);
            let (v_tag, v_val) = ctx.lower_to_tag_value(pair[1]);
            emit_static_add(
                ctx,
                map_op,
                &adder,
                torajs_rc::collection_kind::COLLECTION_MAP,
                [k_tag, k_val, v_tag, v_val],
            );
        }
    }
    finish_static_adder(ctx, adder);
    true
}

fn lower_map_clone(
    ctx: &mut LowerCtx<'_>,
    map_op: Operand,
    arg_op: Operand,
    arg_owned: bool,
) -> Operand {
    ctx.emit_drop_value(map_op, Type::Map);
    let cur_block = ctx.cur_block;
    let cloned = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.map_clone, vec![arg_op.clone()]),
        Type::Map,
        None,
    );
    // Owned source temp only — see lower_iterable_init.
    if arg_owned {
        ctx.emit_drop_value(arg_op, Type::Map);
    }
    Operand::Value(cloned)
}

pub(crate) fn lower_set(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Operand {
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
    let arg_owned = ctx.expr_transfers_ownership(*arg0);
    let arg_ty = ctx.operand_ty(&arg_op);
    if arg_ty == Type::Set {
        return lower_set_clone(ctx, set_op, arg_op, arg_owned);
    }
    if let Type::Arr(arr_id) = arg_ty {
        return crate::ssa_lower_new_arr_init::lower_set_from_arr(
            ctx, set_op, arg_op, arg_ty, arg_owned, arr_id,
        );
    }
    lower_iterable_init(
        ctx,
        set_op,
        Type::Set,
        arg_op,
        arg_owned,
        torajs_rc::collection_kind::COLLECTION_SET,
    )
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
    let adder = lower_static_adder(ctx, set_op, torajs_rc::collection_kind::COLLECTION_SET);
    for elem in &elems {
        let (k_tag, k_val) = ctx.lower_to_tag_value(*elem);
        emit_static_add(
            ctx,
            set_op,
            &adder,
            torajs_rc::collection_kind::COLLECTION_SET,
            [k_tag, k_val, Operand::ConstI64(5), Operand::ConstI64(0)],
        );
    }
    finish_static_adder(ctx, adder);
    true
}

/// §24.1.1.1 step 7.a–c for a literal initializer — the one `Get` of
/// the adder, before any entry is added.
///
/// A literal source is not a reason for `Map.prototype.set` to stop
/// being consulted: the static lane's whole benefit is that it can
/// see the entries, not that it may skip the constructor's own
/// algorithm. Reading the adder once here and threading the answer
/// through every add is what the general walk does too, so both
/// lanes read the same one property the same one time. The answer is
/// undefined for the unpatched program, which is the direct kernel
/// call this lane always emitted.
fn lower_static_adder(ctx: &mut LowerCtx<'_>, target: &Operand, kind: i64) -> Operand {
    let target_any = ctx.box_to_any(target.clone());
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.collection_adder_resolve,
            vec![target_any, Operand::ConstI64(kind)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// One literal entry, routed through whatever the resolve answered.
fn emit_static_add(
    ctx: &mut LowerCtx<'_>,
    target: &Operand,
    adder: &Operand,
    kind: i64,
    slots: [Operand; 4],
) {
    let target_any = ctx.box_to_any(target.clone());
    let [k_tag, k_val, v_tag, v_val] = slots;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.collection_add_static,
            vec![
                target_any,
                adder.clone(),
                Operand::ConstI64(kind),
                k_tag,
                k_val,
                v_tag,
                v_val,
            ],
        ),
    );
}

/// The resolve hands back an owned value; a patched constructor's
/// adder is a live cell this scope is the last owner of. The throw
/// check runs after the entries rather than between them — the
/// kernel skips the rest of the literal once a record is pending, so
/// one check covers the whole initializer.
fn finish_static_adder(ctx: &mut LowerCtx<'_>, adder: Operand) {
    ctx.emit_throw_check(None);
    ctx.emit_drop_value(adder, Type::Any);
}

fn lower_set_clone(
    ctx: &mut LowerCtx<'_>,
    set_op: Operand,
    arg_op: Operand,
    arg_owned: bool,
) -> Operand {
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
    // Owned source temp only — see lower_iterable_init.
    if arg_owned {
        ctx.emit_drop_value(arg_op, Type::Set);
    }
    Operand::Value(cloned)
}
