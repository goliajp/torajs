//! `__torajs_arr_sort_cb` fast-path gate + emission — carved out of
//! [`super::try_dispatch`]. Perf Round 5 attack #1 (RFC
//! 20260703-perf-arr-sort-nlogn): a user comparator whose signature
//! matches the element layout exactly routes through a stable
//! O(n log n) merge sort in torajs-arr that calls the comparator
//! back through the closure ABI.

use crate::ast::{BinOp, Expr, ExprId, Stmt};
use crate::ssa::{BlockId, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Emit the `__torajs_arr_sort_cb` helper call when the comparator
/// is eligible (see [`sort_helper_mode`]). Returns `true` when the
/// call was emitted — the caller then skips the inline fallback.
///
/// Closure ABI: the value IS the env pointer; the raw fn address
/// lives at env + `CLOSURE_FN_ADDR_OFF`. FnSig values are the raw fn
/// pointer with no env. After the call an unconditional throw check
/// (target `None` so the intrinsic skip doesn't fire) propagates any
/// pending comparator throw — the helper unwinds to a complete
/// permutation before returning.
pub(super) fn try_emit_sort_helper(
    ctx: &mut LowerCtx<'_>,
    cmp_val: &Option<Operand>,
    cmp_ty: &Option<Type>,
    args: &[ExprId],
    elem_ty: Type,
    arr_ptr: ValueId,
) -> bool {
    let (Some(cv), Some(ct)) = (cmp_val, cmp_ty) else {
        return false;
    };
    let Some(mode) = sort_helper_mode(ctx, *ct, elem_ty) else {
        return false;
    };
    let mode = mode | native_num_bits(ctx, args.first().copied(), elem_ty);
    let (fn_ptr_op, env_op) = match ct {
        Type::Closure(_) => {
            let env = match cv {
                Operand::Value(v) => *v,
                _ => unreachable!("closure value is SSA"),
            };
            let fp = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(
                    Type::Ptr,
                    Operand::Value(env),
                    crate::ssa_lower::CLOSURE_FN_ADDR_OFF,
                ),
                Type::Ptr,
                None,
            );
            (Operand::Value(fp), Operand::Value(env))
        }
        Type::FnSig(_) => (*cv, Operand::ConstPtrNull),
        _ => unreachable!("sort_helper_mode gates on Closure/FnSig"),
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_sort_cb,
            vec![
                Operand::Value(arr_ptr),
                fn_ptr_op,
                env_op,
                Operand::ConstI64(mode),
            ],
        ),
    );
    ctx.emit_throw_check(None);
    true
}

/// `mode` bits 5 / 6 of torajs-arr's `sort_cmp` — compare the slots
/// natively, in the `b - a` direction when bit 6 is set.
const MODE_NUM_NATIVE: i64 = 32;
const MODE_NUM_DESC: i64 = 64;

/// The native-compare bits when the comparator is a capture-free
/// arrow whose whole body is `a - b` (or `b - a`) over its two plain
/// parameters and the elements are numbers: its sign is exactly the
/// order `a > b` gives (`NumCmp` in torajs-arr has the case analysis),
/// so the runtime sorts without calling it or polling for a throw it
/// cannot raise. Every other shape answers 0 and keeps the call.
fn native_num_bits(ctx: &LowerCtx<'_>, cmp_expr: Option<ExprId>, elem_ty: Type) -> i64 {
    if !matches!(elem_ty, Type::I64 | Type::F64) {
        return 0;
    }
    let Some(Expr::Closure { fn_name, captures }) = cmp_expr.map(|e| ctx.ast.get_expr(e)) else {
        return 0;
    };
    if !captures.is_empty() {
        return 0;
    }
    let Some((params, body)) = crate::ast::toplevel_stmts_flat(ctx.ast)
        .into_iter()
        .find_map(|s| match s {
            Stmt::FnDecl {
                name, params, body, ..
            } if name == fn_name => Some((params, body)),
            _ => None,
        })
    else {
        return 0;
    };
    // A lifted arrow carries its env as a leading `__env` parameter.
    let user = match params.split_first() {
        Some((env, rest)) if env.name == "__env" => rest,
        _ => params.as_slice(),
    };
    let [p, q] = user else {
        return 0;
    };
    if p.default.is_some() || q.default.is_some() || p.is_rest || q.is_rest {
        return 0;
    }
    let [Stmt::Return(Some(ret))] = body.as_slice() else {
        return 0;
    };
    let Expr::BinOp {
        op: BinOp::Sub,
        left,
        right,
    } = ctx.ast.get_expr(*ret)
    else {
        return 0;
    };
    match (ctx.ast.get_expr(*left), ctx.ast.get_expr(*right)) {
        (Expr::Ident(l), Expr::Ident(r)) if *l == p.name && *r == q.name => MODE_NUM_NATIVE,
        (Expr::Ident(l), Expr::Ident(r)) if *l == q.name && *r == p.name => {
            MODE_NUM_NATIVE | MODE_NUM_DESC
        }
        _ => 0,
    }
}

/// Gate + `mode` computation for the `__torajs_arr_sort_cb` helper
/// path. Returns `Some(mode)` only when the comparator can be called
/// back through one of the helper's 8 concrete C signatures with the
/// raw slot bits:
///
/// - comparator params are exactly `[elem_ty, elem_ty]` (no Substr→Str
///   materialization or widening needed),
/// - the return type is I64 or F64,
/// - the element occupies a GPR-class 8-byte slot (I64 / Str pointer)
///   or is F64 (FPR-class). Bool and other layouts fall back to the
///   inline path — their C-ABI register class doesn't match a plain
///   i64 parameter.
///
/// `mode` bit layout mirrors torajs-arr/src/sort.rs: bit 0 elem-f64,
/// bit 1 ret-f64, bit 2 has-env (Closure vs FnSig), bit 3 elem-Str
/// (the runtime runs the §23.1.3.30.2 undefined pre-probe before
/// every comparator call — sentinel elements sort last, never
/// reaching the callback).
fn sort_helper_mode(ctx: &LowerCtx<'_>, cmp_ty: Type, elem_ty: Type) -> Option<i64> {
    let sig_id = match cmp_ty {
        Type::Closure(s) | Type::FnSig(s) => s,
        _ => return None,
    };
    let (params, ret) = &ctx.fn_sigs[sig_id.0 as usize];
    if params.len() != 2 || params[0] != elem_ty || params[1] != elem_ty {
        return None;
    }
    let elem_f64 = match elem_ty {
        Type::F64 => true,
        Type::I64 | Type::Str => false,
        _ => return None,
    };
    let ret_f64 = match ret {
        Type::I64 => false,
        Type::F64 => true,
        _ => return None,
    };
    let has_env = matches!(cmp_ty, Type::Closure(_));
    let elem_str = elem_ty == Type::Str;
    Some(
        (elem_f64 as i64)
            | ((ret_f64 as i64) << 1)
            | ((has_env as i64) << 2)
            | ((elem_str as i64) << 3),
    )
}

/// §23.1.3.30 SortIndexedProperties for a receiver that carries a
/// hole (shadow or slot) or an accessor index:
/// the kernel stages `HasProperty` / `Get` per index (a hole is skipped
/// and a getter runs), sorts the staged values, then `Set`s them back
/// and `DeletePropertyOrThrow`s the tail. Both fast paths permute raw
/// slots, which leaves a hole's shadow where it was and never runs an
/// accessor — `[3, , 1].sort()` answered `[1, <hole>, undefined]`, and
/// a `number[]` with a hole a write past the end left kept its present
/// last element hidden under that hole.
///
/// The header test runs once per call, outside any loop. Returns the
/// join block and leaves `cur_block` at the fast path; `None` when
/// nothing was emitted.
///
/// Recorded gap: a bare `FnSig` comparator has no `any` box spelling
/// here, so that shape keeps the fast path.
pub(super) fn emit_exotic_divert(
    ctx: &mut LowerCtx<'_>,
    arr_ptr: ValueId,
    cmp_val: &Option<Operand>,
    cmp_ty: &Option<Type>,
    elem_ty: Type,
) -> Option<BlockId> {
    if matches!(cmp_ty, Some(t) if !matches!(t, Type::Closure(_) | Type::Any)) {
        return None;
    }
    // A hole (shadow or slot) or an accessor index — the one header
    // test the other per-index MOP gates use.
    let exotic = ctx.emit_arr_index_mop_probe(arr_ptr, 0);
    let slow = ctx.f.add_block();
    let fast = ctx.f.add_block();
    let join = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(exotic),
            then_blk: slow,
            else_blk: fast,
        },
    );
    ctx.cur_block = slow;
    // A typed cell's kernel (`sort_typed_holes`) compares its raw
    // slots reboxed per the element kind the cell names.
    if elem_ty != Type::Any {
        ctx.emit_arr_mark_kind(&Operand::Value(arr_ptr));
    }
    let cmp_arg = (*cmp_val).map(|cv| ctx.box_to_any(cv)).unwrap_or_else(|| {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BoxAny(5, Operand::ConstI64(0)),
            Type::Any,
            None,
        );
        Operand::Value(v)
    });
    // The seam's default releases the kernel's +1 on the receiver
    // itself (a generic any drop here would root the cycle and weak
    // worlds in every sorting program); the caller mints its own
    // chaining stake. The comparator's box is a pure encoding
    // (`box_to_any` moves no stake), so it is not released here —
    // `release_cmp_temp` owns the comparator's.
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_sort_exotic,
            vec![Operand::Value(arr_ptr), cmp_arg],
        ),
    );
    ctx.emit_throw_check(None);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join));
    ctx.cur_block = fast;
    Some(join)
}
