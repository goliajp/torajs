//! `__torajs_arr_sort_cb` fast-path gate + emission — carved out of
//! [`super::try_dispatch`]. Perf Round 5 attack #1 (RFC
//! 20260703-perf-arr-sort-nlogn): a user comparator whose signature
//! matches the element layout exactly routes through a stable
//! O(n log n) merge sort in torajs-arr that calls the comparator
//! back through the closure ABI.

use crate::ssa::{InstKind, Operand, Type, ValueId};
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
    elem_ty: Type,
    arr_ptr: ValueId,
) -> bool {
    let (Some(cv), Some(ct)) = (cmp_val, cmp_ty) else {
        return false;
    };
    let Some(mode) = sort_helper_mode(ctx, *ct, elem_ty) else {
        return false;
    };
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
        Type::FnSig(_) => (cv.clone(), Operand::ConstPtrNull),
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
