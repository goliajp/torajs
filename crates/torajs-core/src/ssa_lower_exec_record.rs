//! The exec-record lane: a `re.exec(s)` result held as spans in a
//! stack record instead of a match array.
//!
//! The census (`ast::regex_exec_record`) lists the `let m = re.exec(s)`
//! declarations whose binding is only null-tested, read as `m.index`
//! / `m.input`, indexed by a constant, measured as `m[k].length`, or
//! re-assigned from the same regex. For those the declaration takes
//! this lane: `__torajs_regex_exec_span` writes group `k`'s UTF-16
//! start and end into slots `2k` / `2k + 1` of a record sized by the
//! largest index the program reads, and answers the hit flag, which is
//! what the binding's `I64` slot holds — 0 for a miss, so every null
//! test and truthiness consumer reads it the way the match-length
//! lane's (`ssa_lower_match_len`) is read. A hidden `Str` local beside
//! it keeps a share of the subject: `m[k]` is cut from it on demand
//! and `m.input` hands it back, and it dies with the scope like any
//! local.
//!
//! A read on a miss is the same catchable TypeError the array lane
//! raises for `null[0]`; `m[k].length` on a group that did not
//! participate is the one `undefined.length` raises. Both go through
//! the kernels the array lane's guards call, so the messages match.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;

/// One binding's record: the span buffer, its size in group slots, and
/// the slot of the hidden local holding the subject.
#[derive(Clone, Copy)]
pub(crate) struct ExecRecord {
    pub(crate) buf: ValueId,
    pub(crate) n_slots: u32,
    pub(crate) subject_slot: ValueId,
}

/// `(callee, receiver, argument)` of `recv.exec(arg)`.
fn exec_shape(ctx: &LowerCtx<'_>, e: ExprId) -> Option<(ExprId, ExprId, ExprId)> {
    let Expr::Call { callee, args } = ctx.ast.get_expr(e) else {
        return None;
    };
    let Expr::Member { obj, name } = ctx.ast.get_expr(*callee) else {
        return None;
    };
    match args.as_slice() {
        [arg] if name == "exec" && !matches!(ctx.ast.get_expr(*arg), Expr::Spread { .. }) => {
            Some((*callee, *obj, *arg))
        }
        _ => None,
    }
}

/// The record behind `e` when it names a binding held as one.
fn record_of(ctx: &LowerCtx<'_>, e: ExprId) -> Option<ExecRecord> {
    let Expr::Ident(n) = ctx.ast.get_expr(e) else {
        return None;
    };
    let local = ctx.locals.get(n)?;
    ctx.exec_records.get(&local.slot).copied()
}

/// `x[k]` over a record binding, with `k` inside the record.
fn record_index(ctx: &LowerCtx<'_>, e: ExprId) -> Option<(ExecRecord, ExprId, u32)> {
    let Expr::Index { obj, index } = ctx.ast.get_expr(e) else {
        return None;
    };
    let rec = record_of(ctx, *obj)?;
    let Expr::Number(n) = ctx.ast.get_expr(*index) else {
        return None;
    };
    if *n < 0.0 || n.fract() != 0.0 || *n >= f64::from(rec.n_slots) {
        return None;
    }
    Some((rec, *obj, *n as u32))
}

/// `let m = re.exec(s)` the census listed, on a RegExp receiver: bind
/// `m` to the hit flag of an exec written into a fresh record.
pub(crate) fn try_lower_let(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    type_ann: Option<&String>,
    init: ExprId,
) -> bool {
    if type_ann.is_some() {
        return false;
    }
    let Some((callee, obj, arg)) = exec_shape(ctx, init) else {
        return false;
    };
    let Some(&n_slots) = ctx.ast.regex_exec_record_callees.get(&callee) else {
        return false;
    };
    if ctx.expr_types.get(&obj) != Some(&crate::check::Type::RegExp) {
        return false;
    }
    let cur_depth = ctx.scope_stack.len() - 1;
    let buf = ctx.f.append_inst(
        BlockId(0),
        InstKind::AllocaBytes(16 * u64::from(n_slots)),
        Type::Ptr,
        Some("__exec_span"),
    );
    // `@` keeps the hidden name out of the program's namespace
    let subject = format!("{name}@exec");
    crate::ssa_lower_stmt_let_decl_general::bind_let_slot(
        ctx,
        &subject,
        Type::Str,
        Operand::ConstPtrNull,
        false,
        false,
        cur_depth,
    );
    let subject_slot = ctx.locals[&subject].slot;
    let rec = ExecRecord {
        buf,
        n_slots,
        subject_slot,
    };
    let hit = emit_exec(ctx, rec, obj, arg);
    crate::ssa_lower_stmt_let_decl_general::bind_let_slot(
        ctx,
        name,
        Type::I64,
        hit,
        false,
        false,
        cur_depth,
    );
    let slot = ctx.locals[name].slot;
    ctx.exec_records.insert(slot, rec);
    // an element read off the binding may answer the `undefined`
    // sentinel, and its consumers (typeof, eq, `.length`) key on the
    // same registry the array lane fills
    ctx.nullable_arr_lets.insert(name.to_string());
    true
}

/// `m = re.exec(t)` over a record binding: refill the record and the
/// hit flag. The assignment's value is the flag.
pub(crate) fn try_assign(ctx: &mut LowerCtx<'_>, name: &str, value: ExprId) -> Option<Operand> {
    let slot = ctx.locals.get(name)?.slot;
    let rec = *ctx.exec_records.get(&slot)?;
    let (_, obj, arg) = exec_shape(ctx, value)?;
    let hit = emit_exec(ctx, rec, obj, arg);
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(hit, Operand::Value(slot), 0));
    Some(hit)
}

/// Run `obj.exec(arg)` into `rec` and answer the hit flag. The record
/// takes its own share of the subject, releasing the previous one.
fn emit_exec(ctx: &mut LowerCtx<'_>, rec: ExecRecord, obj: ExprId, arg: ExprId) -> Operand {
    let recv = ctx.lower_expr(obj);
    assert_eq!(
        ctx.operand_ty(&recv),
        Type::RegExp,
        "exec record receiver lowered to a non-RegExp operand"
    );
    crate::ssa_lower_nullable_guard::emit_undefable_heap_guard(ctx, obj, &recv);
    let (s, s_owned) = crate::ssa_lower_call_regex_methods::lower_haystack(ctx, arg);
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.exec_record.regex_exec_span,
            vec![
                recv,
                s,
                Operand::Value(rec.buf),
                Operand::ConstI64(i64::from(rec.n_slots)),
            ],
        ),
        Type::I64,
        None,
    );
    // a frozen lastIndex under `g` / `y` is a recorded TypeError
    if s_owned {
        ctx.emit_throw_check_owned(None, s, Type::Str);
    } else {
        ctx.emit_throw_check(None);
        ctx.emit_rc_inc(s);
    }
    let old = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(rec.subject_slot), 0),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(s, Operand::Value(rec.subject_slot), 0),
    );
    ctx.emit_drop_value(Operand::Value(old), Type::Str);
    Operand::Value(hit)
}

/// `m.index` / `m.input` over a record binding, and `m[k].length`.
pub(crate) fn try_member(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    name: &str,
) -> Option<Operand> {
    if name == "length"
        && let Some((rec, m, k)) = record_index(ctx, obj)
    {
        emit_hit_guard(ctx, m);
        let (start, end) = load_span(ctx, rec, k);
        // `undefined.length` for a group that did not participate
        let missing = ctx.cmp(IPred::Slt, start, Operand::ConstI64(0));
        let fid = ctx.intrinsics.str_null_check;
        emit_cold_throw(ctx, missing, fid, |ctx| {
            let cell = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::GlobalRef(STR_UNDEF_CELL_SYM.to_string()),
                Type::Str,
                None,
            );
            Operand::Value(cell)
        });
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(crate::ssa::BinOp::Sub, end, start),
            Type::I64,
            None,
        );
        return Some(Operand::Value(len));
    }
    let rec = record_of(ctx, obj)?;
    match name {
        "index" => {
            emit_hit_guard(ctx, obj);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(rec.buf), 0),
                Type::I64,
                None,
            );
            Some(Operand::Value(v))
        }
        "input" => {
            emit_hit_guard(ctx, obj);
            let s = load_subject(ctx, rec);
            ctx.emit_rc_inc(s);
            ctx.owned_member_reads.insert(eid);
            Some(s)
        }
        _ => None,
    }
}

/// `m[k]` over a record binding, read as a value: a fresh string of
/// the group, or the Str `undefined` where it did not participate —
/// the same two values the array's element held.
pub(crate) fn try_index(ctx: &mut LowerCtx<'_>, eid: ExprId) -> Option<Operand> {
    let (rec, m, k) = record_index(ctx, eid)?;
    emit_hit_guard(ctx, m);
    let (start, end) = load_span(ctx, rec, k);
    let subject = load_subject(ctx, rec);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.exec_record.str_group_span,
            vec![subject, start, end],
        ),
        Type::Str,
        None,
    );
    ctx.owned_member_reads.insert(eid);
    Some(Operand::Value(v))
}

/// `parseInt(m[k], radix)` over a record binding, from `lower_parse_int`
/// — which both spellings of the callee share, and which only gets the
/// call once the name is known to still mean the builtin (a module
/// writing the global never reaches it). The group is parsed in place
/// from the subject's span:
/// no Str is cut for it and none is dropped after. Order is the array
/// lane's: the read and its miss guard, the radix, the extra
/// arguments. A group that did not participate is the value
/// `undefined`, so the kernel parses the string `"undefined"` under
/// the radix given — NaN below radix 31, a number from there on.
pub(crate) fn try_parse_int(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let (rec, m, k) = record_index(ctx, args[0])?;
    emit_hit_guard(ctx, m);
    let (start, end) = load_span(ctx, rec, k);
    let subject = load_subject(ctx, rec);
    let radix_undef = args.len() >= 2
        && matches!(
            ctx.expr_types.get(&args[1]),
            Some(crate::check::Type::Undefined)
        );
    let r = if args.len() >= 2 && !radix_undef {
        let raw_r = ctx.lower_expr(args[1]);
        crate::ssa_lower_parse_int_radix::to_int32(ctx, args[1], raw_r)
    } else {
        Operand::ConstI64(0)
    };
    for &a in args.iter().skip(2) {
        let _ = ctx.lower_expr(a);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.exec_record.num_parse_int_span,
            vec![subject, start, end, r],
        ),
        Type::F64,
        None,
    );
    Some(Operand::Value(v))
}

fn load_span(ctx: &mut LowerCtx<'_>, rec: ExecRecord, k: u32) -> (Operand, Operand) {
    let at = |ctx: &mut LowerCtx<'_>, off: u64| {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(rec.buf), off),
            Type::I64,
            None,
        );
        Operand::Value(v)
    };
    let base = 16 * u64::from(k);
    (at(ctx, base), at(ctx, base + 8))
}

fn load_subject(ctx: &mut LowerCtx<'_>, rec: ExecRecord) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(rec.subject_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(v)
}

/// The array lane's null guard for a read through `m`: a miss raises
/// the TypeError `null[0]` does, whatever the checker narrowed.
fn emit_hit_guard(ctx: &mut LowerCtx<'_>, m: ExprId) {
    let hit = ctx.lower_expr(m);
    let miss = ctx.cmp(IPred::Eq, hit, Operand::ConstI64(0));
    let fid = ctx.intrinsics.arr_null_check;
    emit_cold_throw(ctx, miss, fid, |_| Operand::ConstPtrNull);
}

/// When `cond`, call the guard kernel `fid` on `arg` — which records
/// the throw — and propagate it; the hot path branches around.
fn emit_cold_throw(
    ctx: &mut LowerCtx<'_>,
    cond: Operand,
    fid: FuncId,
    arg: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) {
    let cold = ctx.f.add_block();
    let cont = ctx.f.add_block();
    let cur = ctx.cur_block;
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond,
            then_blk: cold,
            else_blk: cont,
        },
    );
    ctx.cur_block = cold;
    let arg = arg(ctx);
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(fid, vec![arg]));
    ctx.emit_throw_check(None);
    let end = ctx.cur_block;
    ctx.f.set_term(end, Terminator::Br(cont));
    ctx.cur_block = cont;
}
