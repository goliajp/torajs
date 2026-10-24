//! The catch slots that admit `null` or `undefined`: a `null` or an
//! `undefined` parameter, and a function type with either beside it
//! (`(() => number) | null`).
//!
//! Each binds in an `any` slot. A `null` or `undefined` annotation
//! parses to a null pointer, which has no room for the value a raise
//! hands in; a function slot spells `null` as the null pointer and
//! holds a function as its box, so a nullable one would need two
//! representations in one slot, and one spelling for `null` and
//! `undefined`. The `any` slot holds each value as it is. What the
//! type does not describe is the TypeError every typed catch raises.

use super::ir::{and, append, guard, icmp, or};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

#[derive(Clone, Copy)]
pub(super) enum Nullish {
    Null,
    Undefined,
    /// A function type with `null` or `undefined`: the normalized
    /// `__nullable(...)` spelling no longer says which, so both pass.
    Fn,
}

/// Which of the nullish slots a parameter of `e_ty`, written `ann`, is.
pub(super) fn slot(e_ty: Type, ann: &str) -> Option<Nullish> {
    match (e_ty, ann) {
        (_, "null") => Some(Nullish::Null),
        (_, "undefined") => Some(Nullish::Undefined),
        (Type::Closure(_), a) if a.starts_with("__nullable(") => Some(Nullish::Fn),
        _ => None,
    }
}

/// Take the pending throw into the `any` slot of the parameter `p`.
pub(super) fn take(ctx: &mut LowerCtx, p: &str, ann: &str, slot: Nullish) -> Operand {
    let (take_tag, take) = (ctx.intrinsics.throw_take_tag, ctx.intrinsics.throw_take);
    let tag = super::ir::call(ctx, take_tag, None);
    let v = super::ir::call(ctx, take, Some(p));
    let ok = match slot {
        Nullish::Null => is_null(ctx, tag, v),
        Nullish::Undefined => icmp(ctx, IPred::Eq, tag, super::TAG_UNDEF),
        Nullish::Fn => {
            let null = is_null(ctx, tag, v);
            let undef = icmp(ctx, IPred::Eq, tag, super::TAG_UNDEF);
            or(ctx, null, undef)
        }
    };
    if let Nullish::Fn = slot {
        return take_fn(ctx, (tag, v), ok);
    }
    let bad = ctx.f.add_block();
    guard(ctx, ok, bad);
    let admitted = ctx.cur_block;
    ctx.cur_block = bad;
    super::reject(ctx, p, ann, (tag, v));
    ctx.cur_block = admitted;
    boxed(ctx, tag, v)
}

/// `null` under its own tag, or a null pointer under the heap tag (a
/// nullable object that holds `null`), as `__torajs_anyv_box_from_pair`
/// reads the pair.
fn is_null(ctx: &mut LowerCtx, tag: ValueId, v: ValueId) -> ValueId {
    let null = icmp(ctx, IPred::Eq, tag, super::TAG_NULL);
    let heap = icmp(ctx, IPred::Eq, tag, super::TAG_HEAP);
    let zero = icmp(ctx, IPred::Eq, v, 0);
    let null_ptr = and(ctx, heap, zero);
    or(ctx, null, null_ptr)
}

/// The nullable function slot: `null` and `undefined` as they are,
/// anything else through the check of a function slot.
fn take_fn(ctx: &mut LowerCtx, (tag, v): (ValueId, ValueId), nullish: ValueId) -> Operand {
    let boxed = boxed(ctx, tag, v);
    let (test, join) = (ctx.f.add_block(), ctx.f.add_block());
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(nullish),
            then_blk: join,
            else_blk: test,
        },
    );
    ctx.cur_block = test;
    super::cast::check_fn(ctx, boxed);
    let tb = ctx.cur_block;
    ctx.f.set_term(tb, Terminator::Br(join));
    ctx.cur_block = join;
    boxed
}

fn boxed(ctx: &mut LowerCtx, tag: ValueId, v: ValueId) -> Operand {
    let pair = vec![Operand::Value(tag), Operand::Value(v)];
    let any_box = ctx.intrinsics.any_box;
    Operand::Value(append(ctx, InstKind::Call(any_box, pair), Type::Any))
}
