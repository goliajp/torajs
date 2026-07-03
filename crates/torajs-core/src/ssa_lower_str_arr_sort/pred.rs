//! `prev > cur` predicate emission for the inline insertion-sort
//! path — carved out of [`super::emit_insertion_sort`]. With a user
//! comparator the callee Operand is invoked and its return widened
//! to a Bool predicate; without one, the ES §23.1.3.30 default
//! SortCompare (ToString + code-unit lex compare) is emitted via the
//! `*_to_str` intrinsics feeding `str_locale_compare`.

use crate::ssa::{FPred, IPred, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Emit the `cmp(prev, cur) > 0` (or default-compare `prev > cur`)
/// predicate in the current block and return its Bool value.
pub(super) fn emit_sort_pred(
    ctx: &mut LowerCtx<'_>,
    cmp_val: &Option<Operand>,
    cmp_ty: &Option<Type>,
    prev: ValueId,
    cur: ValueId,
    elem_ty: Type,
) -> ValueId {
    match (cmp_val, cmp_ty) {
        (Some(cv), Some(ct)) => {
            let cmp_ret = ctx.call_fn_value(
                cv.clone(),
                *ct,
                vec![Operand::Value(prev), Operand::Value(cur)],
            );
            let cmp_ret_ty = ctx.f.value_type(cmp_ret);
            match cmp_ret_ty {
                Type::F64 => ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::FCmp(FPred::Ogt, Operand::Value(cmp_ret), Operand::ConstF64(0.0)),
                    Type::Bool,
                    None,
                ),
                _ => ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::ICmp(IPred::Sgt, Operand::Value(cmp_ret), Operand::ConstI64(0)),
                    Type::Bool,
                    None,
                ),
            }
        }
        _ => {
            // ES §23.1.3.30 SortCompare with no comparator =
            // ToString each operand then code-unit lex compare.
            // Pre-Spec-fix wedge used numeric `prev > cur` which
            // ordered `[10, 2]` as `[2, 10]` — disagrees with bun.
            // For each numeric / bool element, route through the
            // existing `*_to_str` intrinsics so the resulting Str
            // operands feed the same `str_locale_compare`
            // (bytewise; see runtime doc) the Str arm uses. Obj /
            // Arr / etc fall back to the legacy pointer ICmp —
            // no `*_to_str` exists for them yet and the spec
            // result (`"[object Object]"`-tied tie-break) is
            // niche enough to leave behind a follow-up.
            let to_str = |ctx: &mut LowerCtx, v, ty: Type| match ty {
                Type::Str | Type::Substr => Some(v),
                Type::I64 => Some(ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.i64_to_str, vec![Operand::Value(v)]),
                    Type::Str,
                    None,
                )),
                Type::F64 => Some(ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.f64_to_str, vec![Operand::Value(v)]),
                    Type::Str,
                    None,
                )),
                Type::Bool => Some(ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.bool_to_str, vec![Operand::Value(v)]),
                    Type::Str,
                    None,
                )),
                _ => None,
            };
            let prev_s = to_str(ctx, prev, elem_ty);
            let cur_s = to_str(ctx, cur, elem_ty);
            if let (Some(ps), Some(cs)) = (prev_s, cur_s) {
                let r = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.str_locale_compare,
                        vec![Operand::Value(ps), Operand::Value(cs)],
                    ),
                    Type::I64,
                    None,
                );
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::ICmp(IPred::Sgt, Operand::Value(r), Operand::ConstI64(0)),
                    Type::Bool,
                    None,
                )
            } else {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::ICmp(IPred::Sgt, Operand::Value(prev), Operand::Value(cur)),
                    Type::Bool,
                    None,
                )
            }
        }
    }
}
