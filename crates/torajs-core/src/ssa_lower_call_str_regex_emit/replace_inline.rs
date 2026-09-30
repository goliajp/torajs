//! The `s.replace(re, cb)` / `s.replaceAll(re, cb)` walk in the
//! program's own code — the lane `ast::regex_replace_inline` admits.
//!
//! The kernel lane builds a string for the match and one per capture
//! on every match, calls the callback through its pointer and copies
//! its answer into a buffer that becomes the result at the end. Here
//! the loop is emitted around `__torajs_regex_replace_iter_next`: each
//! match's spans land in a stack record, the match and the captures
//! become static-header stack views of the subject (the split loop's
//! borrow, `ssa_lower_stmt_for_of_split_iter`), the callback's view
//! twin is called directly — an ordinary `Call` the inliner can fold
//! into the loop — and its answer is appended onto the result cell in
//! place. Nothing is allocated per match but the callback's own
//! answer.
//!
//! ```text
//! init(iter, re, s); throw check
//! out = alloc_capacity(len(s) + 16); tail = 0
//! header: hit = next(iter, spans, n); br hit ? body : after
//! body:   out = append_substr(out, s[tail..start])
//!         view[k] = s[span k], or the Substr undefined for a group
//!           that did not participate
//!         r = twin(env, argc, view…[, start, s]); throw check
//!         out = append(out, r); drop r; tail = end; br header
//! after:  close(iter); out = append_substr(out, s[tail..])
//! ```
//!
//! The subject is held for the loop (the callback may release the
//! binding it came from), the result cell and the iterator are parked
//! for the throw path, and the lastIndex contract is the kernel's
//! (`torajs-regex` `replace_iter.rs`).

use crate::ast::{Expr, ExprId};
use crate::ssa::{
    BinOp as SsaBinOp, BlockId, FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId,
};
use crate::ssa_lower::{LowerCtx, count_capture_groups};
use crate::ssa_lower_intrinsics_substr::SUBSTR_UNDEF_CELL_SYM;

/// `REPLACE_ITER_SIZE` of `torajs-regex/src/regex/replace_iter.rs`.
const REPLACE_ITER_SIZE: u64 = 96;
/// `FLAG_STATIC_LITERAL << 48` — the packed header of a stack view,
/// the word the split loop writes; the drop of such a view is a no-op.
const HDR_STATIC_LITERAL: i64 = 0x0004i64 << 48;

/// The shape the typed callback ABI serves — `[Str; N+1]` (the match
/// and N captures) or `[Str; N+1, number, string]` (plus position and
/// input) — as (captures declared, names the tail). §22.2.6.11 step
/// 14.j calls the replacer with «matched, captures…, position, string»
/// and a function declaring fewer parameters never sees the rest, so a
/// callback may name fewer captures than the pattern has; naming the
/// tail pins every position, so it must then name them all. A capture
/// is a Str slot (the undefined sentinel for a group that did not
/// take part). Nine captures is the ABI's width.
pub(crate) fn typed_callback_shape(params: &[Type], n_caps: usize) -> Option<(usize, bool)> {
    if n_caps > 9 {
        return None;
    }
    let n = params.len();
    if n >= 3
        && params[n - 2] == Type::I64
        && params[n - 1] == Type::Str
        && params[..n - 2].iter().all(|t| t == &Type::Str)
    {
        return (n - 3 == n_caps).then_some((n_caps, true));
    }
    // an empty list is a replacer that only needs a constant
    if params.iter().all(|t| t == &Type::Str) {
        let declared = n.saturating_sub(1);
        return (declared <= n_caps).then_some((declared, false));
    }
    None
}

/// `s.replace(re, cb)` on the lane, or `None` for the kernel: the
/// census admitted the call, the callback's signature is the typed
/// ABI's shape and its view twin is declared. `env` is the lowered
/// callback (its minted env), released here after the walk.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    recv_op: &Operand,
    re_op: &Operand,
    args: &[ExprId],
    env: &Operand,
) -> Option<Operand> {
    let ast = ctx.ast;
    let twin = ast.regex_replace_inline_callees.get(&callee)?;
    let Type::Closure(sig_id) = ctx.operand_ty(env) else {
        return None;
    };
    let (params, ret_ty) = ctx.fn_sigs[sig_id.0 as usize].clone();
    if ret_ty != Type::Str {
        return None;
    }
    let pattern = match ast.get_expr(args[0]) {
        Expr::Regex { pattern, .. } => pattern.as_str(),
        Expr::Ident(name) => crate::ast::toplevel_regex_literal(ast, name)?.0,
        _ => return None,
    };
    let n_caps = count_capture_groups(pattern);
    let (declared, has_off_input) = typed_callback_shape(&params, n_caps)?;
    let fid = *ctx.fn_table.get(twin)?;
    let plan = Plan {
        fid,
        n_slots: declared + 1,
        has_off_input,
        // §22.2.6.11 step 14.j: «matched, p1..pn, position, string»
        argc: (n_caps + 3) as i64,
        cb: args[1],
    };
    Some(emit_walk(ctx, &plan, recv_op, re_op, env))
}

struct Plan {
    fid: FuncId,
    /// Groups the record holds: the match and the captures declared.
    n_slots: usize,
    has_off_input: bool,
    argc: i64,
    cb: ExprId,
}

fn alloca_bytes(ctx: &mut LowerCtx<'_>, n: u64, name: &str) -> ValueId {
    ctx.f
        .append_inst(BlockId(0), InstKind::AllocaBytes(n), Type::Ptr, Some(name))
}

fn load_i64(ctx: &mut LowerCtx<'_>, ptr: ValueId, off: u64) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(ptr), off),
        Type::I64,
        None,
    );
    Operand::Value(v)
}

fn sub(ctx: &mut LowerCtx<'_>, a: Operand, b: Operand) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Sub, a, b),
        Type::I64,
        None,
    );
    Operand::Value(v)
}

/// Write the Substr layout of `parent[offset .. offset + len)` into the
/// 32-byte stack record `view`, with the static header.
fn write_view(
    ctx: &mut LowerCtx<'_>,
    view: ValueId,
    parent: &Operand,
    offset: Operand,
    len: Operand,
) {
    let fields = [
        (Operand::ConstI64(HDR_STATIC_LITERAL), 0),
        (len, 8),
        (parent.clone(), 16),
        (offset, 24),
    ];
    for (v, off) in fields {
        ctx.f
            .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(view), off));
    }
}

/// The view at `view`, or the Substr `undefined` sentinel when the
/// group's start `gs` is negative — through `slot`, so the two arms
/// meet in one Substr-typed value.
fn select_view(ctx: &mut LowerCtx<'_>, slot: ValueId, gs: Operand, view: ValueId) -> Operand {
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(view), Operand::Value(slot), 0),
    );
    let missing = ctx.cmp(IPred::Slt, gs, Operand::ConstI64(0));
    let miss_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: missing,
            then_blk: miss_blk,
            else_blk: join_blk,
        },
    );
    let undef = ctx.f.append_inst(
        miss_blk,
        InstKind::GlobalRef(SUBSTR_UNDEF_CELL_SYM.to_string()),
        Type::Substr,
        None,
    );
    ctx.f.append_void(
        miss_blk,
        InstKind::Store(Operand::Value(undef), Operand::Value(slot), 0),
    );
    ctx.f.set_term(miss_blk, Terminator::Br(join_blk));
    ctx.cur_block = join_blk;
    let v = ctx.f.append_inst(
        join_blk,
        InstKind::Load(Type::Substr, Operand::Value(slot), 0),
        Type::Substr,
        None,
    );
    Operand::Value(v)
}

/// `out = kernel(out, piece)` through the result slot; `kernel`
/// consumes the reference the slot held.
fn append_into(ctx: &mut LowerCtx<'_>, out_slot: ValueId, kernel: FuncId, piece: Operand) {
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(out_slot), 0),
        Type::Str,
        None,
    );
    let grown = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(kernel, vec![Operand::Value(out), piece]),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(grown), Operand::Value(out_slot), 0),
    );
}

fn emit_walk(
    ctx: &mut LowerCtx<'_>,
    plan: &Plan,
    recv_op: &Operand,
    re_op: &Operand,
    env: &Operand,
) -> Operand {
    let subject = recv_op.clone();
    let init = ctx.intrinsics.exec_record.regex_replace_iter_init;
    let next = ctx.intrinsics.exec_record.regex_replace_iter_next;
    let close = ctx.intrinsics.exec_record.regex_replace_iter_close;
    let append_substr = ctx.intrinsics.exec_record.str_append_substr;
    let append = ctx.intrinsics.str_append;
    // the loop's own share of the subject: every view borrows it, and
    // the callback may release the binding it was read from
    ctx.emit_rc_inc(subject.clone());
    let subject_tok = ctx.push_throw_temp(subject.clone(), Type::Str);
    let env_tok = ctx.park_owned_temp(plan.cb, env);
    let iter = alloca_bytes(ctx, REPLACE_ITER_SIZE, "__replace_iter");
    let spans = alloca_bytes(ctx, 16 * plan.n_slots as u64, "__replace_spans");
    let gap = alloca_bytes(ctx, 32, "__replace_gap");
    let views: Vec<ValueId> = (0..plan.n_slots)
        .map(|_| alloca_bytes(ctx, 32, "__replace_view"))
        .collect();
    let input_view = plan
        .has_off_input
        .then(|| alloca_bytes(ctx, 32, "__replace_input"));
    let arg_slots: Vec<ValueId> = (0..plan.n_slots)
        .map(|_| ctx.alloca(Type::Substr, Some("__replace_arg")))
        .collect();
    let out_slot = ctx.alloca(Type::Str, Some("__replace_out"));
    let tail_slot = ctx.alloca(Type::I64, Some("__replace_tail"));

    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            init,
            vec![Operand::Value(iter), re_op.clone(), subject.clone()],
        ),
    );
    // a frozen lastIndex under `g` / `y` is a recorded TypeError
    ctx.emit_throw_check(None);
    let slen = crate::ssa_lower_str::load_str_or_substr_length(ctx, subject.clone(), Type::Str);
    if let Some(iv) = input_view {
        write_view(ctx, iv, &subject, Operand::ConstI64(0), slen.clone());
    }
    let cap = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, slen.clone(), Operand::ConstI64(16)),
        Type::I64,
        None,
    );
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.exec_record.str_alloc_capacity,
            vec![Operand::Value(cap)],
        ),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(out), Operand::Value(out_slot), 0),
    );
    let out_tok = ctx
        .push_throw_typed_slot(out_slot, Type::Str)
        .expect("a Str slot parks");
    let close_tok = ctx.push_throw_close(close, iter);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(tail_slot), 0),
    );

    let header = ctx.f.add_block();
    let body = ctx.f.add_block();
    let after = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    ctx.cur_block = header;
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            next,
            vec![
                Operand::Value(iter),
                Operand::Value(spans),
                Operand::ConstI64(plan.n_slots as i64),
            ],
        ),
        Type::I64,
        None,
    );
    let found = ctx.cmp(IPred::Ne, Operand::Value(hit), Operand::ConstI64(0));
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: found,
            then_blk: body,
            else_blk: after,
        },
    );

    ctx.cur_block = body;
    let start = load_i64(ctx, spans, 0);
    let end = load_i64(ctx, spans, 8);
    let tail = load_i64(ctx, tail_slot, 0);
    let gap_len = sub(ctx, start.clone(), tail.clone());
    write_view(ctx, gap, &subject, tail, gap_len);
    append_into(ctx, out_slot, append_substr, Operand::Value(gap));
    let mut argv: Vec<Operand> = vec![env.clone(), Operand::ConstI64(plan.argc)];
    for (k, &view) in views.iter().enumerate() {
        let gs = load_i64(ctx, spans, 16 * k as u64);
        let ge = load_i64(ctx, spans, 16 * k as u64 + 8);
        let len = sub(ctx, ge, gs.clone());
        write_view(ctx, view, &subject, gs.clone(), len);
        if k == 0 {
            argv.push(Operand::Value(view));
            continue;
        }
        // §22.2.6.11 step 14.g: a group that did not participate is
        // `undefined`, the immortal Substr sentinel. A slot diamond,
        // which select formation folds to a csel after the egraph
        argv.push(select_view(ctx, arg_slots[k], gs, view));
    }
    if let Some(iv) = input_view {
        argv.push(start);
        argv.push(Operand::Value(iv));
    }
    let ret = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(plan.fid, argv),
        Type::Str,
        None,
    );
    // §22.2.6.11 step 14.j — a callback that throws ends the walk
    ctx.emit_throw_check(Some(plan.fid));
    append_into(ctx, out_slot, append, Operand::Value(ret));
    ctx.emit_drop_value(Operand::Value(ret), Type::Str);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(end, Operand::Value(tail_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = after;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(close, vec![Operand::Value(iter)]),
    );
    ctx.pop_throw_temp(close_tok);
    let tail = load_i64(ctx, tail_slot, 0);
    let rest = sub(ctx, slen, tail.clone());
    write_view(ctx, gap, &subject, tail, rest);
    append_into(ctx, out_slot, append_substr, Operand::Value(gap));
    ctx.pop_throw_temp(out_tok);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(out_slot), 0),
        Type::Str,
        None,
    );
    ctx.unpark_owned_temp(env_tok);
    ctx.release_owned_temp(plan.cb, env);
    ctx.pop_throw_temp(subject_tok);
    ctx.emit_drop_value(subject, Type::Str);
    Operand::Value(result)
}
