//! Inline `<Substr>.trim()` for a Latin-1 parent: both ends scanned
//! in place, the result written as an inline view into the caller's
//! stack buffer. The call it replaces cost more than the one or two
//! bytes a field usually has to skip.

mod ends;

use torajs_rc::FLAG_STATIC_LITERAL;
use torajs_str::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1};
use torajs_str::substr::{
    FLAG_SUBSTR_INLINE, SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF,
};

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_rc_emit::emit_rc_inc_in_fn;
use ends::scan_ends;

// the header word packs count @0, tag @4, flags @6
const HDR_FLAGS_SHIFT: u32 = 48;

/// Where the trim of `recv` goes.
#[derive(Clone, Copy)]
pub(crate) enum Out {
    /// An inline view written into this 32-byte stack buffer, holding
    /// a stake in the parent unless the parent is a static literal,
    /// which is never counted.
    View(ValueId),
    /// Only the trimmed length, stored into this `i64` slot: no view
    /// is written and the parent is neither retained nor released.
    Length(ValueId),
}

/// The trim of `recv`, delivered as `out` says. A Latin-1 parent is
/// scanned in place from both ends against the Latin-1 projection of
/// TrimString's set (TAB..CR, SP, NBSP); a UTF-16 parent takes
/// `__torajs_substr_trim_into`.
pub(crate) fn emit(ctx: &mut LowerCtx<'_>, recv: Operand, out: Out) {
    let entry = ctx.cur_block;
    let parent = load(ctx, entry, Type::Ptr, recv, SUBSTR_PARENT_OFF);
    let off = load(ctx, entry, Type::I64, recv, SUBSTR_OFFSET_OFF);
    let len = load(ctx, entry, Type::I64, recv, SUBSTR_LEN_OFF);
    let hdr = load(ctx, entry, Type::I64, Operand::Value(parent), 0);
    let lat_mask = i64::from(STR_FLAG_IS_LATIN1) << HDR_FLAGS_SHIFT;
    let lat = bin(
        ctx,
        entry,
        BinOp::And,
        Operand::Value(hdr),
        Operand::ConstI64(lat_mask),
    );
    let is_lat = cmp(
        ctx,
        entry,
        IPred::Ne,
        Operand::Value(lat),
        Operand::ConstI64(0),
    );

    let slow = ctx.f.add_block();
    let fast = ctx.f.add_block();
    let join = ctx.f.add_block();
    cond_br(ctx, entry, is_lat, fast, slow);

    emit_kernel(ctx, slow, recv, out);
    ctx.f.set_term(slow, Terminator::Br(join));

    let p_int = ctx.f.append_inst(
        fast,
        InstKind::PtrToInt(Operand::Value(parent)),
        Type::I64,
        None,
    );
    let data = bin(
        ctx,
        fast,
        BinOp::Add,
        Operand::Value(p_int),
        Operand::ConstI64(STR_DATA_OFF as i64),
    );
    let start = bin(
        ctx,
        fast,
        BinOp::Add,
        Operand::Value(data),
        Operand::Value(off),
    );
    let base = ctx.f.append_inst(
        fast,
        InstKind::IntToPtr(Operand::Value(start)),
        Type::Ptr,
        None,
    );
    let (lo_end, hi_end, emit) = scan_ends(ctx, fast, base, len);
    let new_len = bin(
        ctx,
        emit,
        BinOp::Sub,
        Operand::Value(hi_end),
        Operand::Value(lo_end),
    );
    let buf = match out {
        Out::View(buf) => buf,
        Out::Length(slot) => {
            store(ctx, emit, Operand::Value(new_len), slot, 0);
            ctx.f.set_term(emit, Terminator::Br(join));
            ctx.cur_block = join;
            return;
        }
    };
    let new_off = bin(
        ctx,
        emit,
        BinOp::Add,
        Operand::Value(off),
        Operand::Value(lo_end),
    );
    let inline_hdr = i64::from(FLAG_SUBSTR_INLINE) << HDR_FLAGS_SHIFT;
    store(ctx, emit, Operand::ConstI64(inline_hdr), buf, 0);
    store(ctx, emit, Operand::Value(new_len), buf, SUBSTR_LEN_OFF);
    store(ctx, emit, Operand::Value(parent), buf, SUBSTR_PARENT_OFF);
    store(ctx, emit, Operand::Value(new_off), buf, SUBSTR_OFFSET_OFF);
    // the matching drop reads the same bit before it decrements
    let static_mask = i64::from(FLAG_STATIC_LITERAL) << HDR_FLAGS_SHIFT;
    let st = bin(
        ctx,
        emit,
        BinOp::And,
        Operand::Value(hdr),
        Operand::ConstI64(static_mask),
    );
    let counted = cmp(
        ctx,
        emit,
        IPred::Eq,
        Operand::Value(st),
        Operand::ConstI64(0),
    );
    let retain = ctx.f.add_block();
    cond_br(ctx, emit, counted, retain, join);
    emit_rc_inc_in_fn(ctx.f, retain, Operand::Value(parent), Type::Str);
    ctx.f.set_term(retain, Terminator::Br(join));

    ctx.cur_block = join;
}

/// The trimmed length of the Latin-1 run `[base, base + len)` stored
/// into `slot`, scanned from the current block, which is left at the
/// block after the store.
pub(crate) fn emit_latin1_length(
    ctx: &mut LowerCtx<'_>,
    base: ValueId,
    len: ValueId,
    slot: ValueId,
) {
    let (lo_end, hi_end, done) = scan_ends(ctx, ctx.cur_block, base, len);
    let n = bin(
        ctx,
        done,
        BinOp::Sub,
        Operand::Value(hi_end),
        Operand::Value(lo_end),
    );
    store(ctx, done, Operand::Value(n), slot, 0);
    ctx.cur_block = done;
}

/// The UTF-16 arm: the kernel writes the view, into `out`'s buffer or,
/// when only the length is wanted, into a scratch buffer released as
/// soon as its length is read.
pub(crate) fn emit_kernel(ctx: &mut LowerCtx<'_>, at: BlockId, recv: Operand, out: Out) {
    let (buf, slot) = match out {
        Out::View(buf) => (buf, None),
        Out::Length(slot) => {
            let tmp = ctx
                .f
                .append_inst(at, InstKind::AllocaBytes(32), Type::Substr, None);
            (tmp, Some(slot))
        }
    };
    let trim_into = ctx.intrinsics.substr_trim_into;
    ctx.f.append_void(
        at,
        InstKind::Call(trim_into, vec![recv, Operand::Value(buf)]),
    );
    if let Some(slot) = slot {
        let len = load(ctx, at, Type::I64, Operand::Value(buf), SUBSTR_LEN_OFF);
        store(ctx, at, Operand::Value(len), slot, 0);
        let drop = ctx.intrinsics.substr_drop;
        ctx.f
            .append_void(at, InstKind::Call(drop, vec![Operand::Value(buf)]));
    }
}

fn load(ctx: &mut LowerCtx<'_>, at: BlockId, ty: Type, ptr: Operand, off: usize) -> ValueId {
    ctx.f
        .append_inst(at, InstKind::Load(ty, ptr, off as u64), ty, None)
}

fn store(ctx: &mut LowerCtx<'_>, at: BlockId, v: Operand, ptr: ValueId, off: usize) {
    ctx.f
        .append_void(at, InstKind::Store(v, Operand::Value(ptr), off as u64));
}

fn bin(ctx: &mut LowerCtx<'_>, at: BlockId, op: BinOp, a: Operand, b: Operand) -> ValueId {
    ctx.f
        .append_inst(at, InstKind::BinOp(op, a, b), Type::I64, None)
}

fn cmp(ctx: &mut LowerCtx<'_>, at: BlockId, p: IPred, a: Operand, b: Operand) -> ValueId {
    ctx.f
        .append_inst(at, InstKind::ICmp(p, a, b), Type::Bool, None)
}

fn cond_br(ctx: &mut LowerCtx<'_>, at: BlockId, c: ValueId, then_blk: BlockId, else_blk: BlockId) {
    ctx.f.set_term(
        at,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        },
    );
}
