//! What the body of an async function holds at a suspension point, for
//! the frame's trace (`super`).

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_temp_scratch::ThrowTemp;

/// What the body owns at a suspension point: what a cancelled frame's
/// exit after it releases (`cancel`), that is the owned locals, the
/// temps parked for a throw, what the `finally` bodies around it keep
/// aside ([`aside`]) and the operand the suspension keeps. The frame's
/// trace reports them while the function waits there. Left out: a
/// substring view (no cycle passes through one, and it has no
/// universal header to read), and whatever the throw path releases by
/// another protocol than a drop of the value it holds (capture boxes,
/// class cells, relocation slots).
pub(super) fn held(
    ctx: &mut LowerCtx<'_>,
    kept: Option<(Operand, Type)>,
    aside: &[Operand],
) -> Vec<Operand> {
    let traced = |ty: Type| !ty.is_copy() && ty != Type::Substr;
    let mut slots: Vec<(ValueId, Type)> = (ctx.locals.iter())
        .filter(|(name, info)| {
            traced(info.ty)
                && !info.moved
                && !ctx.stack_alloced_locals.contains(*name)
                && !ctx.boxed_noncopy_lets.contains(*name)
                && !(info.ty == Type::Any
                    && crate::ssa_lower_closure::class_sentinel_name(ctx, name))
        })
        .map(|(_, info)| (info.slot, info.ty))
        .collect();
    let temps = ctx.temps.throw_live.iter().flatten();
    let mut held: Vec<Operand> = aside.to_vec();
    for t in temps {
        match t {
            ThrowTemp::Value(op, ty) if traced(*ty) => held.push(*op),
            ThrowTemp::Slot(slot, ty) if traced(*ty) => slots.push((*slot, *ty)),
            _ => {}
        }
    }
    slots.sort_by_key(|(s, _)| s.0);
    slots.dedup_by_key(|(s, _)| s.0);
    let cb = ctx.cur_block;
    for (slot, ty) in slots {
        let load = InstKind::Load(ty, Operand::Value(slot), 0);
        held.push(Operand::Value(ctx.f.append_inst(cb, load, ty, None)));
    }
    if let Some((op, ty)) = kept
        && traced(ty)
    {
        held.push(op);
    }
    held
}

/// What the `finally` bodies around a suspension point keep aside, as
/// heap pointers: a pending return (its flag set) and each throw set
/// aside (active, under the heap tag). Whether a slot holds one is
/// known only at run time, so each is read into a word of its own
/// before the park, null where the slot holds none; the trace reports
/// the word, a cancelled frame's exit releases it, and both skip a
/// null.
pub(super) fn aside(ctx: &mut LowerCtx<'_>) -> Vec<Operand> {
    if ctx.pending.finally_throws.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let ret = ctx.f.ret;
    if let (Some(flag), Some(slot)) = (ctx.pending.return_flag, ctx.pending.return_slot)
        && !ret.is_copy()
        && ret != Type::Substr
    {
        let set = load(ctx, Type::Bool, flag);
        out.push(read_if(ctx, 0, &[set], ret, slot));
    }
    let levels = ctx.pending.finally_throws.clone();
    for (level, [active, tag, value]) in levels.into_iter().enumerate() {
        let active = load(ctx, Type::I64, active);
        let active = icmp(ctx, IPred::Ne, active, 0);
        let tag = load(ctx, Type::I64, tag);
        let heap = icmp(ctx, IPred::Eq, tag, ANY_HEAP_TAG);
        out.push(read_if(ctx, level + 1, &[active, heap], Type::I64, value));
    }
    out
}

/// The tag of a thrown heap value (`__torajs_anyv_box_from_pair`).
const ANY_HEAP_TAG: i64 = 4;

fn load(ctx: &mut LowerCtx<'_>, ty: Type, slot: ValueId) -> Operand {
    let k = InstKind::Load(ty, Operand::Value(slot), 0);
    Operand::Value(ctx.f.append_inst(ctx.cur_block, k, ty, None))
}

fn icmp(ctx: &mut LowerCtx<'_>, pred: IPred, v: Operand, c: i64) -> Operand {
    let k = InstKind::ICmp(pred, v, Operand::ConstI64(c));
    Operand::Value(ctx.f.append_inst(ctx.cur_block, k, Type::Bool, None))
}

/// The word `slot` holds (read as `ty`) where every `conds` holds,
/// else null, through aside word `key` of the function.
fn read_if(
    ctx: &mut LowerCtx<'_>,
    key: usize,
    conds: &[Operand],
    ty: Type,
    slot: ValueId,
) -> Operand {
    let word = aside_word(ctx, key);
    let store = |ctx: &mut LowerCtx<'_>, v| {
        let k = InstKind::Store(v, Operand::Value(word), 0);
        ctx.f.append_void(ctx.cur_block, k);
    };
    store(ctx, Operand::ConstI64(0));
    let join = ctx.f.add_block();
    for &cond in conds {
        let then_blk = ctx.f.add_block();
        let cb = ctx.cur_block;
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk: join,
            },
        );
        ctx.cur_block = then_blk;
    }
    let v = load(ctx, ty, slot);
    store(ctx, v);
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(join));
    ctx.cur_block = join;
    load(ctx, Type::Ptr, word)
}

fn aside_word(ctx: &mut LowerCtx<'_>, key: usize) -> ValueId {
    let words = &mut ctx.pending.aside_words;
    if words.len() <= key {
        words.resize(key + 1, None);
    }
    if let Some(w) = words[key] {
        return w;
    }
    let w = ctx.alloca_in_entry(Type::Ptr, Some("__aside"));
    ctx.pending.aside_words[key] = Some(w);
    w
}
