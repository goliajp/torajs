//! The run-time split of a direct call that the checker retargeted to
//! a clone taking a `string` parameter as `any`.
//!
//! The clone binds whatever the caller passed (§10.2.1.3), so `null`
//! stays `null` in the body. Most such arguments are strings at run
//! time, though, and a heap string can take the typed body unchanged:
//! each widened argument is tested inline (a cell, tagged `Str`, not a
//! substring view); when all of them pass the call goes to the
//! original function with the cells borrowed as they are, otherwise
//! to the clone as passed. An inline short string goes to the clone
//! too: the typed body would need a heap copy of it, which costs more
//! than the clone's any-lane reads. Typed callers never reach this
//! split.

use crate::ast::ExprId;
use crate::ssa::{BinOp, BlockId, FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_anyv_inline::{bin, branch, emit_bits, emit_is_cell, inst};

/// The substring-view bit of a Str cell's header flags.
const SUBSTR_VIEW: i64 = 1 << 10;

/// The two faces of a split call.
struct Split {
    orig: FuncId,
    /// Signature positions the clone widened from `string` to `any`.
    slots: Vec<usize>,
    /// The typed answer is boxed: the clone answers `any`.
    box_ret: bool,
}

/// Emit the split of call `eid` for `argv` (already aligned to the
/// retargeted `clone`'s signature) and answer the joined result in
/// `clone`'s return type. `None`, with nothing emitted, when the call
/// was not retargeted to an any-widened clone, or `clone` is not its
/// original with some `string` parameters widened to `any` and the
/// same return face.
pub(super) fn emit(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    clone: FuncId,
    argv: &[Operand],
) -> Option<Operand> {
    let split = split_slots(ctx, eid, clone, argv.len())?;
    let rt = ctx.f_ret_type_hint(clone);
    let slot = (rt != Type::Void).then(|| ctx.alloca_in_entry(rt, None));
    let wide_blk = ctx.f.add_block();
    let join = ctx.f.add_block();
    let mut narrow_argv = argv.to_vec();
    for &p in &split.slots {
        narrow_argv[p] = Operand::Value(emit_str_cell_test(ctx, &argv[p], wide_blk));
    }
    let typed = super::emit_direct_call(ctx, split.orig, narrow_argv);
    let typed = if split.box_ret {
        ctx.box_to_any(typed)
    } else {
        typed
    };
    store(ctx, slot, typed);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join));
    ctx.cur_block = wide_blk;
    let any = super::emit_direct_call(ctx, clone, argv.to_vec());
    store(ctx, slot, any);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join));
    ctx.cur_block = join;
    Some(match slot {
        Some(s) => {
            let load = InstKind::Load(rt, Operand::Value(s), 0);
            Operand::Value(ctx.f.append_inst(join, load, rt, None))
        }
        None => Operand::ConstPtrNull,
    })
}

/// Leave for `wide_blk` unless `av` is a heap Str cell that is not a
/// substring view; answer the cell as a borrowed `Str` in the block
/// that continues.
fn emit_str_cell_test(ctx: &mut LowerCtx<'_>, av: &Operand, wide_blk: BlockId) -> ValueId {
    let hdr_blk = ctx.f.add_block();
    let next = ctx.f.add_block();
    let bits = emit_bits(ctx, av);
    let is_cell = emit_is_cell(ctx, bits);
    branch(ctx, is_cell, hdr_blk, wide_blk);
    // bytes 4..8 of the cell: type_tag u16 | flags u16
    ctx.cur_block = hdr_blk;
    let p = inst(ctx, InstKind::IntToPtr(Operand::Value(bits)), Type::Ptr);
    let word = inst(
        ctx,
        InstKind::Load(Type::I64, Operand::Value(p), 4),
        Type::I64,
    );
    let mask = 0xFFFF | SUBSTR_VIEW << 16;
    let probe = inst(
        ctx,
        bin(BinOp::And, word, Operand::ConstI64(mask)),
        Type::I64,
    );
    let str_tag = Operand::ConstI64(torajs_rc::Tag::Str as i64);
    let is_str = inst(
        ctx,
        InstKind::ICmp(IPred::Eq, Operand::Value(probe), str_tag),
        Type::Bool,
    );
    branch(ctx, is_str, next, wide_blk);
    ctx.cur_block = next;
    inst(ctx, InstKind::IntToPtr(Operand::Value(bits)), Type::Str)
}

/// The split of call `eid`, when its retarget `clone` is the any-widen
/// clone of a lowered original that differs only in `string` slots.
/// The any-widen lane names its clone `<callee>$$anyw<slots>`; any
/// other retarget (a generic instance, whose original is never
/// lowered) has no typed body to split to.
fn split_slots(ctx: &LowerCtx<'_>, eid: ExprId, clone: FuncId, argc: usize) -> Option<Split> {
    let (base, _) = ctx.call_retargets.get(&eid)?.rsplit_once("$$anyw")?;
    let orig = *ctx.fn_table.get(base)?;
    let wide = &ctx.fn_sigs[ctx.fn_sig_ids.get(&clone)?.0 as usize].0;
    let narrow = &ctx.fn_sigs[ctx.fn_sig_ids.get(&orig)?.0 as usize].0;
    if clone == orig || wide.len() != narrow.len() || wide.len() != argc {
        return None;
    }
    let mut slots = Vec::new();
    for (p, (w, n)) in wide.iter().zip(narrow).enumerate() {
        match (w, n) {
            (Type::Any, Type::Str) => slots.push(p),
            _ if w == n => {}
            _ => return None,
        }
    }
    let (rt, ro) = (ctx.f_ret_type_hint(clone), ctx.f_ret_type_hint(orig));
    let box_ret = match (rt, ro) {
        _ if rt == ro => false,
        (Type::Any, r) if r != Type::Void => true,
        _ => return None,
    };
    (!slots.is_empty()).then_some(Split {
        orig,
        slots,
        box_ret,
    })
}

fn store(ctx: &mut LowerCtx<'_>, slot: Option<ValueId>, v: Operand) {
    if let Some(s) = slot {
        ctx.f
            .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(s), 0));
    }
}
