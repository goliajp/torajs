//! The guarded fill loop for a `while` that walks a string — the
//! shape of a tokenizer:
//!
//! ```ignore
//!   let i = 0;
//!   while (i < src.length) {
//!     const c = src.charCodeAt(i);
//!     if (c === 32) { i = i + 1 }
//!     else if (c >= 48 && c <= 57) {
//!       let v = 0;
//!       while (i < src.length) { /* digits */ i = i + 1 }
//!       out.push(new Token(NUM, v));
//!     } else { out.push(new Token(OP, c)); i = i + 1 }
//!   }
//! ```
//!
//! The bare fill lanes (the parent module) take a body of pushes and a
//! final step, which fixes the trip count, so their reserve covers
//! every append. Here the pushes sit in branches and the counter moves
//! in several places, so nothing bounds the appends. The loop reads
//! the room its buffer has at entry, and each append tests the running
//! length against it and grows on a cold edge when there is none
//! (`ssa_lower_arr_prereserve::guard`). The room read at entry is all
//! the appends trust, so it should cover the walk: when the buffer
//! holds less than `k * src.length` more elements, `k` being the most
//! appends one trip can make (the busier arm of every `if`, nothing
//! inside an inner loop), the walk reserves that first, enough for
//! every append of a loop that steps its counter by one. A cell that
//! the array pool hands back keeps its buffer, so a walk that runs
//! again on an array of the same size reserves nothing. The forecast is
//! an upper bound (`8 * k` bytes a character for a word-sized element,
//! 16 for a two-field instance held in place) that a sparse walk, one
//! that appends at a few characters of a long text, never fills, so it
//! stops at `RESERVE_MAX` elements; a walk that outgrows that grows on
//! the cold edges, by doubling. A count past what a u32 capacity holds
//! reserves nothing. The cold edge raises the RangeError of §10.4.2.4
//! at the append that would take the length past that.
//!
//! The appends hold the array's length word back until the loop ends,
//! and keep the running length in a register. Both hold only while
//! the body cannot run code or raise: code could read the array, push
//! to it behind the running length or shift it, and a throw would
//! leave by an edge that never writes the length back, so a `catch`
//! would read a stale one. So every statement and expression in the
//! body is on an allowlist (the `allow` module): numeric and boolean
//! locals past their declaration, literals and arithmetic over them,
//! `s.length` and `s.charCodeAt(e)` on the string the loop's test
//! reads (the test raises on a nullish one on its first trip, before
//! any append) or on one that cannot be nullish, the array's own
//! pushes of such values (or of an instance built in place from them),
//! `if`, inner `while` loops with their own `break` / `continue`, and
//! nothing else. Anything off it takes the plain loop. The cold edges
//! that can raise (a lock bit, a length past the longest) write the
//! length back first.

mod allow;

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_CAP_HEAD_OFF, ARR_LEN_OFF, LocalInfo, LowerCtx};
use crate::ssa_lower_arr_prereserve::WalkRaise;

use super::lower_loop;
use allow::{Names, body_decls, first_push_receiver, is_string_ident, walk_stmt};

/// Lower `while (cond) body`, `counter` known to be 0 at entry, as a
/// walk when it qualifies; answers whether it did.
pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, counter: &str, cond: ExprId, body: &Stmt) -> bool {
    let Some(plan) = plan(ctx, counter, cond, body) else {
        return false;
    };
    let arr = plan.arr;
    let info = ctx.locals[&arr];
    let Type::Arr(id) = info.ty else {
        unreachable!("a walk's receiver is an array")
    };
    let elem = ctx.arr_layouts[id.0 as usize];
    let k = crate::ssa_lower::inline_elem::elem_stride(ctx.struct_layouts, elem) / 8;
    let cell = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    // an array holding its class in place has no deque head (see
    // `ssa_lower_arr_prereserve::guard`)
    let inline = matches!(elem, Type::Inline(_));
    let headless = inline || ctx.proofs.arr_head_pinned();
    // an array holding its class in place never carries a lock bit
    // (see `inline_push`); a debug compiler and the strict switch
    // still test
    let test_lock =
        !inline || cfg!(debug_assertions) || crate::ssa_lower_pass_2_5::value_class_check::strict();
    if let Some(s) = plan.src {
        reserve_forecast(ctx, cell, s, plan.per_trip, k, headless);
    }
    let state = ctx.emit_guarded_state(cell, k, headless, test_lock);
    ctx.prereserve.unchecked_for.insert(arr.clone(), state);
    ctx.prereserve.walk_raise = Some(WalkRaise::at_entry(ctx));
    lower_loop(ctx, cond, body);
    ctx.emit_prereserved_len_writeback(state);
    ctx.prereserve.unchecked_for.remove(&arr);
    ctx.prereserve.walk_raise = None;
    true
}

/// The most elements a walk reserves in front of the loop: 32 MiB of
/// word-sized elements.
const RESERVE_MAX: i64 = 1 << 22;

/// Reserve room for `per_trip` more `k`-slot elements a character of
/// the string in the local `src`, when the buffer of `cell` holds less,
/// and for no more than `RESERVE_MAX` of them. The test against the
/// buffer takes the whole forecast, so a buffer that holds it costs no
/// more than the test; the cap is applied on the edge that reserves.
/// The string is read raw, with no nullish test of its own: a nullish
/// one raises in the loop's own test, which comes after, and the
/// `undefined` cells are strings whose length word reads 9.
fn reserve_forecast(
    ctx: &mut LowerCtx<'_>,
    cell: ValueId,
    src: LocalInfo,
    per_trip: i64,
    k: u64,
    headless: bool,
) {
    let cb = ctx.cur_block;
    let s = InstKind::Load(src.ty, Operand::Value(src.slot), 0);
    let s = Operand::Value(ctx.f.append_inst(cb, s, src.ty, None));
    let null = ctx.cmp(IPred::Eq, s, Operand::ConstPtrNull);
    let read_blk = ctx.f.add_block();
    let grow_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: null,
            then_blk: join_blk,
            else_blk: read_blk,
        },
    );
    ctx.cur_block = read_blk;
    let len = ctx
        .f
        .append_inst(read_blk, InstKind::Load(Type::I32, s, 8), Type::I32, None);
    let len = InstKind::ZExtI32ToI64(Operand::Value(len));
    let len = Operand::Value(ctx.f.append_inst(read_blk, len, Type::I64, None));
    let target = reserve_target(ctx, cell, len, per_trip, k, headless, Past::All);
    let cb = ctx.cur_block;
    let packed = InstKind::Load(Type::I64, Operand::Value(cell), ARR_CAP_HEAD_OFF);
    let packed = Operand::Value(ctx.f.append_inst(cb, packed, Type::I64, None));
    let cap = ctx.bin(
        BinOp::And,
        packed,
        Operand::ConstI64(0xffff_ffff),
        Type::I64,
    );
    let short = ctx.cmp(IPred::Ult, cap, target);
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: short,
            then_blk: grow_blk,
            else_blk: join_blk,
        },
    );
    ctx.cur_block = grow_blk;
    // min(len, RESERVE_MAX / per_trip) by arithmetic: the lowering may
    // not form a select
    let most = Operand::ConstI64(RESERVE_MAX / per_trip);
    let room = ctx.bin(BinOp::Sub, most, len, Type::I64);
    let sign = ctx.bin(BinOp::AShr, room, Operand::ConstI64(63), Type::I64);
    let over = ctx.bin(BinOp::And, room, sign, Type::I64);
    let len = ctx.bin(BinOp::Add, len, over, Type::I64);
    let target = reserve_target(ctx, cell, len, per_trip, k, headless, Past::None);
    let ty = ctx.f.value_type(cell);
    let reserve = InstKind::Call(
        ctx.intrinsics.arr_reserve,
        vec![Operand::Value(cell), target],
    );
    let cb = ctx.cur_block;
    ctx.f.append_inst(cb, reserve, ty, None);
    ctx.f.set_term(cb, Terminator::Br(join_blk));
    ctx.cur_block = join_blk;
}

/// What [`reserve_target`] answers for a count past what a u32
/// capacity holds.
enum Past {
    /// all ones, which every capacity is below
    All,
    /// 0, which reserves nothing
    None,
}

/// The slots, counted from the buffer's start, that `bound` trips of
/// `per_trip` appends of `k`-slot elements need on top of the elements
/// already there and the deque head in front of them, unless the head
/// is known to be 0; `past` says what it is when that is past what a
/// u32 capacity holds.
fn reserve_target(
    ctx: &mut LowerCtx<'_>,
    arr: ValueId,
    bound: Operand,
    per_trip: i64,
    k: u64,
    headless: bool,
    past: Past,
) -> Operand {
    let a = Operand::Value(arr);
    let cb = ctx.cur_block;
    let len = InstKind::Load(Type::I64, a, ARR_LEN_OFF);
    let len = ctx.f.append_inst(cb, len, Type::I64, None);
    let added = match per_trip {
        1 => bound,
        n => ctx.bin(BinOp::Mul, bound, Operand::ConstI64(n), Type::I64),
    };
    let total = ctx.bin(BinOp::Add, Operand::Value(len), added, Type::I64);
    let slots = match k {
        1 => total,
        k => ctx.bin(BinOp::Mul, total, Operand::ConstI64(k as i64), Type::I64),
    };
    let slots = match headless {
        true => slots,
        false => {
            let x8 = ctx.emit_arr_head_x8(a);
            let head = ctx.bin(BinOp::LShr, x8, Operand::ConstI64(3), Type::I64);
            ctx.bin(BinOp::Add, slots, head, Type::I64)
        }
    };
    // all ones when the count is past, else 0, by arithmetic: the
    // lowering may not form a select
    let max = Operand::ConstI64(i64::from(u32::MAX));
    let room = ctx.bin(BinOp::Sub, max, slots, Type::I64);
    let over = ctx.bin(BinOp::AShr, room, Operand::ConstI64(63), Type::I64);
    match past {
        Past::All => ctx.bin(BinOp::Or, slots, over, Type::I64),
        Past::None => {
            let fits = ctx.bin(BinOp::Xor, over, Operand::ConstI64(-1), Type::I64);
            ctx.bin(BinOp::And, slots, fits, Type::I64)
        }
    }
}

/// What a body that qualifies is made of.
struct Plan {
    arr: String,
    per_trip: i64,
    /// the local the loop test reads the length of, when it is one
    src: Option<LocalInfo>,
}

fn plan(ctx: &LowerCtx<'_>, counter: &str, cond: ExprId, body: &Stmt) -> Option<Plan> {
    let ast = ctx.ast;
    let Expr::BinOp {
        op: crate::ast::BinOp::Lt,
        left,
        right,
    } = ast.get_expr(cond)
    else {
        return None;
    };
    if !matches!(ast.get_expr(*left), Expr::Ident(n) if n == counter) {
        return None;
    }
    let Expr::Member { obj, name } = ast.get_expr(*right) else {
        return None;
    };
    if name != "length" || !is_string_ident(ctx, *obj) {
        return None;
    }
    let Expr::Ident(src_name) = ast.get_expr(*obj) else {
        unreachable!("is_string_ident answers for an identifier")
    };
    let src = ctx.locals.get(src_name).copied().filter(|l| !l.boxed);
    let c = ctx.locals.get(counter)?;
    if c.boxed || !matches!(c.ty, Type::I64 | Type::I32 | Type::F64) {
        return None;
    }
    let arr = first_push_receiver(ast, body)?;
    let mut decls = Vec::new();
    body_decls(body, &mut decls);
    let names = Names {
        counter,
        arr: &arr,
        src: src_name,
        decls,
    };
    let per_trip = walk_stmt(ctx, body, &names, 0)?;
    let info = ctx.locals.get(&arr)?;
    let Type::Arr(id) = info.ty else {
        return None;
    };
    if per_trip == 0 || info.boxed || ctx.arr_layouts[id.0 as usize] == Type::Any {
        return None;
    }
    Some(Plan { arr, per_trip, src })
}
