//! `<Array<T>>.{findIndex|findLastIndex|find|findLast|some|every}` short-
//! circuit predicate iteration pulled out of
//! [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch as chunk-10
//! of the `Expr::Call` god-arm decomp (chunks 1-9 = Arr higher-order +
//! Map dispatch + Set dispatch + Arr.push + Number instance methods +
//! bare-name globals + Str regex methods + Number namespace + Array.from).
//!
//! Six methods share the same per-element predicate-iter shape:
//! - `findIndex` / `findLastIndex` — return i64 index (-1 default)
//! - `find` / `findLast` — return elem_ty (zero/null default)
//! - `some` — return bool (false default, true on first match)
//! - `every` — return bool (true default, false on first miss)
//!
//! `findLastIndex` / `findLast` walk back-to-front (`i = len-1`, step `-1`,
//! cmp `i >= 0`); the rest walk front-to-back. All exit on first match
//! (find* / some) or first miss (every) per ES §23.1.3.{5-10}.
//!
//! Returns `Some(result)` when callee matches `Expr::Member` + one of the
//! 6 method names — even when the receiver is non-array, the original
//! block panicked at SSA lower-time and that behavior is preserved.
//! `None` lets the caller fall through to the next M2/M3/M4 arm below.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower a `xs.<predicate-method>(p, ...)` call. Returns `Some` when
/// dispatched.
pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (obj, name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if !matches!(
        name.as_str(),
        "findIndex" | "findLastIndex" | "find" | "findLast" | "some" | "every"
    ) {
        return None;
    }
    let recv_op = ctx.lower_expr(obj);
    let recv_ty = ctx.operand_ty(&recv_op);
    if !matches!(recv_ty, Type::Arr(_)) {
        panic!("ssa-lower: `.{name}(...)` on non-array receiver type {recv_ty:?}");
    }
    let elem_ty = ctx.arr_layouts[match recv_ty {
        Type::Arr(id) => id.0 as usize,
        _ => unreachable!(),
    }];
    let src_arr = match recv_op {
        Operand::Value(v) => v,
        _ => unreachable!(),
    };
    let fn_val = ctx.lower_expr(args[0]);
    let fn_ty = ctx.operand_ty(&fn_val);
    // S270 — eval-and-drop trailing args (thisArg + any extras) so side-
    // effect expressions fire per ES §23.1.3.X trailing-arg ignore. SSA-
    // emit reads only args[0] (the predicate cb).
    for &t in args.iter().skip(1) {
        let _ = ctx.lower_expr(t);
    }
    let (result_ty, result_slot) = setup_result_slot(ctx, &name, elem_ty);
    let out = emit_predicate_iter(
        ctx,
        &name,
        src_arr,
        elem_ty,
        fn_val,
        fn_ty,
        result_ty,
        result_slot,
    );
    // RFC 20260705 chunk 552 — release owned-shape temps after the
    // loop consumed them (inline arrow's minted env in the cb slot,
    // Call/New-shaped receiver).
    ctx.release_owned_temp(args[0], &fn_val);
    ctx.release_owned_temp(obj, &recv_op);
    Some(out)
}

/// Allocate the result slot in the entry block + store the per-method default
/// (so a no-match walk returns the JS spec default without conditional stores
/// in the loop post-section).
///
/// Result slot:
///   findIndex / findLastIndex → Type::I64 (-1 default)
///   some / every             → Type::Bool (false / true)
///   find / findLast          → elem_ty (zero-init default)
/// For find / findLast the not-found return is the zero / null of the
/// element type — no `T | undefined` since tr lacks union types. For
/// refcounted elements the null-pointer sentinel makes a meaningful
/// check (`r === null`); for primitives the user must verify existence
/// via findIndex first.
fn setup_result_slot(ctx: &mut LowerCtx<'_>, method: &str, elem_ty: Type) -> (Type, ValueId) {
    let is_find = matches!(method, "find" | "findLast");
    let result_ty = if is_find {
        elem_ty
    } else if matches!(method, "findIndex" | "findLastIndex") {
        Type::I64
    } else {
        Type::Bool
    };
    let result_slot = ctx.alloca_in_entry(result_ty, Some("__pred_res"));
    let default_op: Operand = match method {
        "findIndex" | "findLastIndex" => Operand::ConstI64(-1),
        "some" => Operand::ConstBool(false),
        "every" => Operand::ConstBool(true),
        "find" | "findLast" => match elem_ty {
            Type::I64 => Operand::ConstI64(0),
            Type::F64 => Operand::ConstF64(0.0),
            Type::Bool => Operand::ConstBool(false),
            // RC-1 — an Any slot must default to the boxed `undefined`
            // (ES no-match result); raw null-ptr bits are not a valid
            // NaN-box and render as [unknown-any-tag] downstream.
            Type::Any => {
                let undef = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.any_box,
                        vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                    ),
                    Type::Any,
                    None,
                );
                Operand::Value(undef)
            }
            _ => Operand::ConstPtrNull,
        },
        _ => unreachable!(),
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(default_op, Operand::Value(result_slot), 0),
    );
    (result_ty, result_slot)
}

/// Emit the full predicate-iter loop: i_slot init + forward/reverse cmp +
/// body (load elem → call predicate → branch hit/next) + i++ step + after-
/// block result load. Returns the final loaded result operand.
#[allow(clippy::too_many_arguments)]
fn emit_predicate_iter(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    src_arr: ValueId,
    elem_ty: Type,
    fn_val: Operand,
    fn_ty: Type,
    result_ty: Type,
    result_slot: ValueId,
) -> Operand {
    let is_reverse = method == "findLastIndex" || method == "findLast";
    let i_slot = ctx.alloca(Type::I64, Some("__pred_i"));
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    // Forward: i = 0; loop while i < len; step i + 1.
    // Reverse (findLastIndex): i = len - 1; loop while i >= 0; step i - 1.
    let i_init: Operand = if is_reverse {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(SsaBinOp::Sub, Operand::Value(len), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        Operand::Value(v)
    } else {
        Operand::ConstI64(0)
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(i_init, Operand::Value(i_slot), 0),
    );
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    // header
    ctx.cur_block = header_blk;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let cmp = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            if is_reverse { IPred::Sge } else { IPred::Slt },
            Operand::Value(i_now),
            if is_reverse {
                Operand::ConstI64(0)
            } else {
                Operand::Value(len)
            },
        ),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(cmp),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    emit_body_and_step(
        ctx,
        method,
        is_reverse,
        body_blk,
        header_blk,
        after_blk,
        i_slot,
        src_arr,
        elem_ty,
        fn_val,
        fn_ty,
        result_slot,
    );
    ctx.cur_block = after_blk;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(result_ty, Operand::Value(result_slot), 0),
        result_ty,
        None,
    );
    Operand::Value(r)
}

/// Emit the loop body (load elem + call predicate) + hit-vs-next branch +
/// i++ step. Caller has already placed the `cmp` cond-br into body_blk;
/// after this fn returns body's slots are wired and cur_block points at
/// after_blk's predecessor (no, actually we leave cur_block on whatever the
/// final ctx.f.set_term left it — the caller resets to after_blk before
/// loading the result).
#[allow(clippy::too_many_arguments)]
fn emit_body_and_step(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    is_reverse: bool,
    body_blk: crate::ssa::BlockId,
    header_blk: crate::ssa::BlockId,
    after_blk: crate::ssa::BlockId,
    i_slot: ValueId,
    src_arr: ValueId,
    elem_ty: Type,
    fn_val: Operand,
    fn_ty: Type,
    result_slot: ValueId,
) {
    // body — load elem, run predicate
    ctx.cur_block = body_blk;
    let i_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    // T-13.5: head-aware offset for some/every/findIndex.
    // RFC 20260707 chunk 625 — an Any elem reads through the
    // kind-aware borrowed-box helper (typed-behind-Arr<Any> raw
    // slots misread under a raw LoadDyn; same borrow contract).
    let elem = if elem_ty == Type::Any {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_get_any_boxed,
                vec![Operand::Value(src_arr), Operand::Value(i_now2)],
            ),
            Type::Any,
            None,
        )
    } else {
        let (off_base, off) = ctx.emit_arr_slot_byte_offset(
            Operand::Value(src_arr),
            Operand::Value(i_now2),
            3,
            false,
        );
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::LoadDyn(elem_ty, off_base, off),
            elem_ty,
            None,
        )
    };
    // RC-1 (RFC 20260706-test262-bug-corpus) — a Void-ret predicate
    // returns `undefined`; ToBoolean folds every hit test to false
    // (ES §23.1.3.{8-11,30}). Emit the call for its side effects
    // only — consuming the void call's value is the SIGTRAP lane.
    let cb_ret_void = ctx.callback_ret_ty(fn_ty) == Some(Type::Void);
    let pred_op: Operand = if cb_ret_void {
        let _ = ctx.call_fn_value(fn_val, fn_ty, vec![Operand::Value(elem)], 0);
        Operand::ConstBool(false)
    } else {
        let pred_v = ctx.call_fn_value(fn_val, fn_ty, vec![Operand::Value(elem)], 0);
        Operand::Value(pred_v)
    };
    // some + findIndex break on `pred == true`; every breaks on
    // `pred == false`.
    let break_cond = if method == "every" {
        let inv = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, pred_op, Operand::ConstBool(false)),
            Type::Bool,
            None,
        );
        Operand::Value(inv)
    } else {
        pred_op
    };
    let hit_blk = ctx.f.add_block();
    let next_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: break_cond,
            then_blk: hit_blk,
            else_blk: next_blk,
        },
    );
    // hit: write the appropriate result and exit. For find / findLast the
    // elem is the result; refcounted elements get rc_inc'd so the caller's
    // binding owns a ref independent of the source array's slot.
    ctx.cur_block = hit_blk;
    let hit_val: Operand = match method {
        "findIndex" | "findLastIndex" => Operand::Value(i_now2),
        "some" => Operand::ConstBool(true),
        "every" => Operand::ConstBool(false),
        "find" | "findLast" => {
            if elem_ty.is_refcounted() {
                ctx.emit_rc_inc(Operand::Value(elem));
            }
            Operand::Value(elem)
        }
        _ => unreachable!(),
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(hit_val, Operand::Value(result_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    // next: i++ and loop
    ctx.cur_block = next_blk;
    let i_then = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            if is_reverse {
                SsaBinOp::Sub
            } else {
                SsaBinOp::Add
            },
            Operand::Value(i_then),
            Operand::ConstI64(1),
        ),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
}
