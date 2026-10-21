//! The match-length lane's call (`s.match(re)` held as its first
//! match's length + 1, `ssa_lower_match_len`). When the pattern is a
//! literal the compiler synthesized a Latin-1 matcher for, and has
//! neither `g` nor `y`, the call site walks that matcher directly
//! over an ASCII haystack: the answer `__torajs_str_match_len` would
//! reach through its entry, its flag checks and an indirect call.
//!
//! The direct arm is taken on two run-time facts, and anything else
//! takes the kernel:
//!
//! - the haystack's header carries both the Latin-1 and the ASCII bit
//!   (the kernel's own view would scan a Latin-1 payload without the
//!   ASCII bit; that scan stays the kernel's);
//! - the RegExp's `matcher_latin1` is this literal's matcher. Only
//!   `__torajs_regex_compile_baked` stores it, and only
//!   for this pattern and these flags, so the RegExp is unrejected,
//!   non-global, non-sticky and searches exactly as the matcher does.
//!   A RegExp re-initialized by `compile()` carries the runtime's
//!   `None` and falls to the kernel.
//!
//! Without `g` / `y` the kernel neither reads nor writes lastIndex and
//! records no throw, so the direct arm has no throw check.

use torajs_regex::regex::REGEX_MATCHER_LATIN1_OFF;
use torajs_str::layout::{STR_FLAG_IS_ASCII, STR_FLAG_IS_LATIN1, STR_LEN_OFF};

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp, FuncId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_regex_bake::matcher::{matcher_ir, matcher_key};

// the header word packs count @0, tag @4, flags @6
const HDR_FLAGS_SHIFT: u32 = 48;

/// `len + 1` of `recv.match(re)`, `0` for null.
pub(super) fn emit(
    ctx: &mut LowerCtx<'_>,
    recv: Operand,
    re: Operand,
    re_expr: Option<ExprId>,
) -> Operand {
    let len = match re_expr.and_then(|e| direct_matcher(ctx, e)) {
        Some((fid, stride)) => emit_direct(ctx, recv, re, fid, stride),
        None => emit_kernel(ctx, recv, re),
    };
    let held = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::Add, len, Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    Operand::Value(held)
}

/// The Latin-1 matcher of the literal `re_expr` spells or names
/// through a top-level `const`, with the row width it writes, when the
/// literal has neither `g` nor `y`.
fn direct_matcher(ctx: &LowerCtx<'_>, re_expr: ExprId) -> Option<(FuncId, u32)> {
    let ast = ctx.ast;
    let (pattern, flags) = match ast.get_expr(re_expr) {
        Expr::Regex { pattern, flags } => (pattern.as_str(), flags.as_str()),
        Expr::Ident(name) => crate::ast::toplevel_regex_literal(ast, name)?,
        _ => return None,
    };
    if flags.contains(['g', 'y']) {
        return None;
    }
    let fid = *ctx.fn_table.get(&matcher_key(pattern, flags, false))?;
    let stride = matcher_ir(pattern, flags, false)?.stride;
    Some((fid, stride))
}

fn emit_kernel(ctx: &mut LowerCtx<'_>, recv: Operand, re: Operand) -> Operand {
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_match_len, vec![recv, re]),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(len)
}

fn emit_direct(
    ctx: &mut LowerCtx<'_>,
    recv: Operand,
    re: Operand,
    fid: FuncId,
    stride: u32,
) -> Operand {
    let entry = ctx.cur_block;
    let out = ctx
        .f
        .append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let row = ctx.f.append_inst(
        entry,
        InstKind::AllocaBytes(8 * u64::from(stride)),
        Type::Ptr,
        None,
    );
    let ours = ctx.f.add_block();
    let direct = ctx.f.add_block();
    let slow = ctx.f.add_block();
    let join = ctx.f.add_block();

    // a null string slot or a RegExp read before its binding settles
    // takes the kernel, which answers both
    let present = ctx.f.add_block();
    let recv_i = ctx
        .f
        .append_inst(entry, InstKind::PtrToInt(recv), Type::I64, None);
    let re_i = ctx
        .f
        .append_inst(entry, InstKind::PtrToInt(re), Type::I64, None);
    let recv_set = ctx.f.append_inst(
        entry,
        InstKind::ICmp(IPred::Ne, Operand::Value(recv_i), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let both = ctx.f.add_block();
    cond_br(ctx, entry, recv_set, both, slow);
    let re_set = ctx.f.append_inst(
        both,
        InstKind::ICmp(IPred::Ne, Operand::Value(re_i), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    cond_br(ctx, both, re_set, present, slow);
    let entry = present;
    let hdr = ctx
        .f
        .append_inst(entry, InstKind::Load(Type::I64, recv, 0), Type::I64, None);
    let mask = i64::from(STR_FLAG_IS_LATIN1 | STR_FLAG_IS_ASCII) << HDR_FLAGS_SHIFT;
    let bits = ctx.f.append_inst(
        entry,
        InstKind::BinOp(BinOp::And, Operand::Value(hdr), Operand::ConstI64(mask)),
        Type::I64,
        None,
    );
    let ascii = ctx.f.append_inst(
        entry,
        InstKind::ICmp(IPred::Eq, Operand::Value(bits), Operand::ConstI64(mask)),
        Type::Bool,
        None,
    );
    cond_br(ctx, entry, ascii, ours, slow);

    let bound = ctx.f.append_inst(
        ours,
        InstKind::Load(Type::I64, re, REGEX_MATCHER_LATIN1_OFF as u64),
        Type::I64,
        None,
    );
    let sig = ctx.fn_sig_ids[&fid];
    let addr = ctx
        .f
        .append_inst(ours, InstKind::FnAddr(fid), Type::FnSig(sig), None);
    let want = ctx.f.append_inst(
        ours,
        InstKind::PtrToInt(Operand::Value(addr)),
        Type::I64,
        None,
    );
    let same = ctx.f.append_inst(
        ours,
        InstKind::ICmp(IPred::Eq, Operand::Value(bound), Operand::Value(want)),
        Type::Bool,
        None,
    );
    cond_br(ctx, ours, same, direct, slow);

    // the payload follows the header; a Latin-1 Str's length counts bytes
    let base = ctx
        .f
        .append_inst(direct, InstKind::PtrToInt(recv), Type::I64, None);
    let data = ctx.f.append_inst(
        direct,
        InstKind::BinOp(
            BinOp::Add,
            Operand::Value(base),
            Operand::ConstI64(torajs_str::layout::STR_DATA_OFF as i64),
        ),
        Type::I64,
        None,
    );
    let units = ctx.f.append_inst(
        direct,
        InstKind::IntToPtr(Operand::Value(data)),
        Type::Ptr,
        None,
    );
    let n32 = ctx.f.append_inst(
        direct,
        InstKind::Load(Type::I32, recv, STR_LEN_OFF as u64),
        Type::I32,
        None,
    );
    let n = ctx.f.append_inst(
        direct,
        InstKind::ZExtI32ToI64(Operand::Value(n32)),
        Type::I64,
        None,
    );
    // a miss returns -1 and leaves the row alone: with its start slot
    // at 0 the difference below is the kernel's -1 without a branch
    store(ctx, direct, Operand::ConstI64(0), row);
    let end = ctx.f.append_inst(
        direct,
        InstKind::Call(
            fid,
            vec![
                Operand::Value(units),
                Operand::Value(n),
                Operand::ConstI64(0),
                Operand::Value(row),
            ],
        ),
        Type::I64,
        None,
    );
    let start = ctx.f.append_inst(
        direct,
        InstKind::Load(Type::I64, Operand::Value(row), 0),
        Type::I64,
        None,
    );
    let len = ctx.f.append_inst(
        direct,
        InstKind::BinOp(BinOp::Sub, Operand::Value(end), Operand::Value(start)),
        Type::I64,
        None,
    );
    store(ctx, direct, Operand::Value(len), out);
    ctx.f.set_term(direct, Terminator::Br(join));

    ctx.cur_block = slow;
    let len = emit_kernel(ctx, recv, re);
    let tail = ctx.cur_block;
    store(ctx, tail, len, out);
    ctx.f.set_term(tail, Terminator::Br(join));

    ctx.cur_block = join;
    let v = ctx.f.append_inst(
        join,
        InstKind::Load(Type::I64, Operand::Value(out), 0),
        Type::I64,
        None,
    );
    Operand::Value(v)
}

fn store(ctx: &mut LowerCtx<'_>, blk: crate::ssa::BlockId, v: Operand, ptr: crate::ssa::ValueId) {
    ctx.f
        .append_void(blk, InstKind::Store(v, Operand::Value(ptr), 0));
}

fn cond_br(
    ctx: &mut LowerCtx<'_>,
    blk: crate::ssa::BlockId,
    cond: crate::ssa::ValueId,
    then_blk: crate::ssa::BlockId,
    else_blk: crate::ssa::BlockId,
) {
    ctx.f.set_term(
        blk,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk,
            else_blk,
        },
    );
}
