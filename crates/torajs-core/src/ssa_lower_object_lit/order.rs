//! Property creation order of an object literal versus the layout it
//! is stored in.
//!
//! A literal under a declared type (`const a: A = { x: 1, b: "q" }`
//! with `type A = { b: string; x: number }`) is stored in the
//! declared layout, so every typed read of `a.b` stays one load at
//! the declared offset. §10.1.11.1 still enumerates the properties in
//! the order the literal created them, so the cell wears a tag whose
//! layout rows list the slots in that order (one tag per layout and
//! order, minted by the anon stamp pool).
//!
//! A spread copies the source's properties in the source's own
//! order, which is a run-time fact once the source's layout can be
//! created in more than one order: [`retag_for_spread_sources`] picks
//! the result's tag from the sources' tags.

use crate::ast::PropKey;
use crate::ssa::{IPred, InstKind, Operand, StructId, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_CLASS_TAG_OFF};

/// A spread member of the literal and its source cell.
pub(super) struct SpreadSrc {
    pub(super) entry: usize,
    pub(super) op: Operand,
    pub(super) sid: StructId,
    pub(super) omit: Vec<PropKey>,
}

/// Permute `field_tys` / `field_vals` (creation order) into the
/// layout the literal is stored in when it spells the same names in
/// another order: the declared layout the position names, else the
/// declared order the checker recorded as the literal's type (a call
/// lane that does not name the parameter's layout). Answers the
/// creation order as indices into the new order, or `None` when
/// nothing moved. The values are already evaluated, so the
/// evaluation order is untouched.
pub(super) fn to_declared_order(
    ctx: &LowerCtx<'_>,
    eid: crate::ast::ExprId,
    declared_hint: Option<StructId>,
    field_tys: &mut Vec<(PropKey, Type)>,
    field_vals: &mut Vec<Operand>,
) -> Option<Vec<u32>> {
    let target: Vec<PropKey> = match declared_hint {
        Some(h) => ctx
            .struct_layouts
            .get(h.0 as usize)?
            .iter()
            .map(|(n, _)| n.clone())
            .collect(),
        None => match ctx.expr_types.get(&eid)? {
            crate::check::Type::Struct(fs) => fs.iter().map(|(n, _)| n.clone()).collect(),
            _ => return None,
        },
    };
    if target.len() != field_tys.len() {
        return None;
    }
    let order: Vec<u32> = field_tys
        .iter()
        .map(|(n, _)| target.iter().position(|d| d == n).map(|i| i as u32))
        .collect::<Option<_>>()?;
    if is_identity(&order) {
        return None;
    }
    let mut tys = field_tys.clone();
    let mut vals = field_vals.clone();
    for (k, &i) in order.iter().enumerate() {
        tys[i as usize] = field_tys[k].clone();
        vals[i as usize] = field_vals[k];
    }
    *field_tys = tys;
    *field_vals = vals;
    Some(order)
}

pub(super) fn is_identity(order: &[u32]) -> bool {
    order.iter().enumerate().all(|(i, &o)| i as u32 == o)
}

/// The property names a spread of a `layout` cell created in `order`
/// contributes, in that order.
fn spread_names(layout: &[(PropKey, Type)], order: &[u32], omit: &[PropKey]) -> Vec<PropKey> {
    let names: Vec<PropKey> = order
        .iter()
        .map(|&i| layout[i as usize].0.clone())
        .collect();
    crate::ssa_lower_anon_stamp::spread_contribution(&names, omit)
}

/// After the static tag store: for every combination of non-layout
/// orders the varying spread sources can have, compute the result's
/// creation order and, when a source cell carries that combination's
/// tags at run time, overwrite the result's tag.
pub(super) fn retag_for_spread_sources(
    ctx: &mut LowerCtx<'_>,
    obj_ptr: crate::ssa::ValueId,
    sid: StructId,
    entries: &[(PropKey, crate::ast::ExprId)],
    srcs: &[SpreadSrc],
) {
    let choices: Vec<Vec<Vec<u32>>> = srcs
        .iter()
        .map(|s| {
            let layout = &ctx.struct_layouts[s.sid.0 as usize];
            let mut all = vec![(0..layout.len() as u32).collect::<Vec<u32>>()];
            all.extend(ctx.foreign_orders_of(s.sid));
            all
        })
        .collect();
    // the same bound the order scan expands spread literals under: past
    // it the scan recorded no order for this literal, so none is minted
    let total = choices.iter().map(Vec::len).product::<usize>();
    if total == 1 || total > crate::ssa_lower_anon_stamp::MAX_COMBOS {
        return;
    }
    let mut combo = vec![0usize; srcs.len()];
    loop {
        if combo.iter().any(|&c| c != 0) {
            emit_combo(ctx, obj_ptr, sid, entries, srcs, &choices, &combo);
        }
        let Some(k) = (0..combo.len()).find(|&k| combo[k] + 1 < choices[k].len()) else {
            break;
        };
        combo[k] += 1;
        for c in combo.iter_mut().take(k) {
            *c = 0;
        }
    }
}

fn emit_combo(
    ctx: &mut LowerCtx<'_>,
    obj_ptr: crate::ssa::ValueId,
    sid: StructId,
    entries: &[(PropKey, crate::ast::ExprId)],
    srcs: &[SpreadSrc],
    choices: &[Vec<Vec<u32>>],
    combo: &[usize],
) {
    // replay the literal's property creation with these source orders
    let mut created: Vec<PropKey> = Vec::new();
    let mut add = |n: PropKey| {
        if !created.contains(&n) {
            created.push(n);
        }
    };
    for (idx, (name, _)) in entries.iter().enumerate() {
        if let Some(k) = srcs.iter().position(|s| s.entry == idx) {
            let s = &srcs[k];
            let layout = &ctx.struct_layouts[s.sid.0 as usize];
            spread_names(layout, &choices[k][combo[k]], &s.omit)
                .into_iter()
                .for_each(&mut add);
        } else {
            add(name.clone());
        }
    }
    let layout = &ctx.struct_layouts[sid.0 as usize];
    let Some(order) = created
        .iter()
        .map(|n| layout.iter().position(|(d, _)| d == n).map(|i| i as u32))
        .collect::<Option<Vec<u32>>>()
    else {
        return;
    };
    // only an order the scan recorded gets a tag, so every tag a cell
    // can wear is one the compile-time walks already route around
    if is_identity(&order) || !ctx.foreign_orders_of(sid).contains(&order) {
        return;
    }
    let tag = ctx
        .anon_stamp_pool
        .borrow_mut()
        .assign_or_get_ordered(sid, &order);
    let mut cond: Option<Operand> = None;
    for (k, s) in srcs.iter().enumerate() {
        if choices[k].len() == 1 {
            continue;
        }
        // the layout's own order is every tag but the foreign-order ones
        // (a class instance of that layout wears its class tag)
        let (pred, wants): (IPred, Vec<u32>) = if combo[k] == 0 {
            let mut pool = ctx.anon_stamp_pool.borrow_mut();
            let ts = choices[k][1..]
                .iter()
                .map(|o| pool.assign_or_get_ordered(s.sid, o))
                .collect();
            (IPred::Ne, ts)
        } else {
            let t = ctx
                .anon_stamp_pool
                .borrow_mut()
                .assign_or_get_ordered(s.sid, &choices[k][combo[k]]);
            (IPred::Eq, vec![t])
        };
        let have = crate::ssa_lower::load_obj_class_tag(ctx, s.op);
        for want in wants {
            let eq = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(pred, Operand::Value(have), Operand::ConstI64(want as i64)),
                Type::Bool,
                None,
            );
            cond = Some(match cond {
                None => Operand::Value(eq),
                Some(c) => Operand::Value(ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BinOp(crate::ssa::BinOp::And, c, Operand::Value(eq)),
                    Type::Bool,
                    None,
                )),
            });
        }
    }
    let Some(cond) = cond else {
        return;
    };
    let store_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond,
            then_blk: store_blk,
            else_blk: done_blk,
        },
    );
    ctx.cur_block = store_blk;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(
            Operand::ConstI64(tag as i64),
            Operand::Value(obj_ptr),
            OBJ_CLASS_TAG_OFF,
        ),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = done_blk;
}
