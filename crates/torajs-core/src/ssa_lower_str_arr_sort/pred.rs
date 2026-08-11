//! `prev > cur` predicate emission for the inline insertion-sort
//! path — carved out of [`super::emit_insertion_sort`]. With a user
//! comparator the callee Operand is invoked and its return widened
//! to a Bool predicate; without one, the ES §23.1.3.30 default
//! SortCompare (ToString + code-unit lex compare) is emitted via the
//! `*_to_str` intrinsics feeding `str_sort_cmp` (undefined-last +
//! null-as-"null" per §23.1.3.30.2 before the byte compare).

use crate::ssa::{FPred, IPred, InstKind, Operand, Terminator, Type, ValueId};
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
            // §23.1.3.30.2 steps 5-8 fire BEFORE the comparator — an
            // undefined element sorts last without ever reaching the
            // callback. The Str slot shapes carry the sentinel repr;
            // an Any slot carries the tag-5 immediate (刀 7 G8a).
            if matches!(elem_ty, Type::Str | Type::Substr) {
                let pre_fid = ctx.intrinsics.str_sort_undef_pre;
                return emit_user_cmp_undef_pre(ctx, cv, ct, prev, cur, pre_fid);
            }
            if matches!(elem_ty, Type::Any) {
                let pre_fid = ctx.intrinsics.any_sort_undef_pre;
                return emit_user_cmp_undef_pre(ctx, cv, ct, prev, cur, pre_fid);
            }
            emit_user_cmp_pred(ctx, cv, ct, prev, cur)
        }
        _ => {
            // ES §23.1.3.30 SortCompare with no comparator =
            // ToString each operand then code-unit lex compare.
            // Pre-Spec-fix wedge used numeric `prev > cur` which
            // ordered `[10, 2]` as `[2, 10]` — disagrees with bun.
            // For each numeric / bool element, route through the
            // existing `*_to_str` intrinsics so the resulting Str
            // operands feed the same `str_locale_compare`
            // (bytewise; see runtime doc) the Str arm uses; Obj /
            // Arr elements tag-4 box through the runtime ToString
            // (刀 7 G8b). Remaining exotic element types fall back
            // to the legacy pointer ICmp.
            // The second bool marks a minted temp the compare must
            // release (RFC 20260705 chunk 551) — the Str/Substr arm
            // answers the element borrow itself and is not dropped.
            let to_str = |ctx: &mut LowerCtx, v, ty: Type| match ty {
                Type::Str | Type::Substr => Some((v, false)),
                Type::I64 => Some((
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.i64_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    ),
                    true,
                )),
                Type::F64 => Some((
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.f64_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    ),
                    true,
                )),
                Type::Bool => Some((
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.bool_to_str, vec![Operand::Value(v)]),
                        Type::Str,
                        None,
                    ),
                    true,
                )),
                // Any elem (Arr<Any> receiver): unbox the NaN-box
                // pair and route through the runtime tag-dispatched
                // ToString — same fresh-owned contract as the
                // coerce_to_str Any arm. Without this arm the
                // fallback ICmp compares raw NaN-box bits (string
                // elements order by pointer value — silent wrong).
                Type::Any => {
                    let tag = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![Operand::Value(v)]),
                        Type::I64,
                        None,
                    );
                    let raw = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![Operand::Value(v)]),
                        Type::I64,
                        None,
                    );
                    Some((
                        ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(
                                ctx.intrinsics.any_to_str,
                                vec![Operand::Value(tag), Operand::Value(raw)],
                            ),
                            Type::Str,
                            None,
                        ),
                        true,
                    ))
                }
                // Obj / Arr elements (typed `Array<Struct>` default
                // sort — 刀 7 G8b): tag-4 box the cell and route the
                // same runtime tag-dispatched ToString, so the user's
                // ToPrimitive hooks fire per §23.1.3.30. Pre-fix these
                // fell to the legacy pointer ICmp and never ToString'd.
                Type::Obj(_) | Type::Arr(_) => {
                    let (tag, value) = ctx.heap_slot_tag_value(Operand::Value(v));
                    Some((
                        ctx.f.append_inst(
                            ctx.cur_block,
                            InstKind::Call(ctx.intrinsics.any_to_str, vec![tag, value]),
                            Type::Str,
                            None,
                        ),
                        true,
                    ))
                }
                _ => None,
            };
            let prev_s = to_str(ctx, prev, elem_ty);
            let cur_s = to_str(ctx, cur, elem_ty);
            // The Any / Obj / Arr arms' ToString runs
            // OrdinaryToPrimitive — a throwing user hook aborts the
            // sort (0-check audit, rotation 130 L3b).
            if matches!(elem_ty, Type::Any | Type::Obj(_) | Type::Arr(_)) {
                ctx.emit_throw_check(None);
            }
            if let (Some((ps, ps_minted)), Some((cs, cs_minted))) = (prev_s, cur_s) {
                let r = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.str_sort_cmp,
                        vec![Operand::Value(ps), Operand::Value(cs)],
                    ),
                    Type::I64,
                    None,
                );
                if ps_minted {
                    ctx.emit_drop_value(Operand::Value(ps), Type::Str);
                }
                if cs_minted {
                    ctx.emit_drop_value(Operand::Value(cs), Type::Str);
                }
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

/// `cmp(prev, cur) > 0` — invoke the user comparator and widen its
/// return to the Bool predicate (F64 return compares against 0.0).
fn emit_user_cmp_pred(
    ctx: &mut LowerCtx<'_>,
    cv: &Operand,
    ct: &Type,
    prev: ValueId,
    cur: ValueId,
) -> ValueId {
    // RFC 20260726-array-elem-width knife 11 — the elements' width and
    // the comparator's parameter width answer to two different classes,
    // and the wiring that ties them is a one-way width edge (elements
    // widen the parameter, never the reverse). So a comparator shared
    // with a fractional array is compiled to take f64 while this
    // receiver's elements are still integers, and handing the slots
    // over unconverted aborted register allocation. The helper fast
    // path refuses this case outright (`sort_helper_mode` requires the
    // parameters to equal the element type) and falls through to here,
    // where nothing converted them either.
    let params = match *ct {
        Type::Closure(s) | Type::FnSig(s) => ctx.fn_sigs[s.0 as usize].0.clone(),
        _ => Vec::new(),
    };
    let args = [prev, cur]
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let op = Operand::Value(*v);
            if params.get(i) == Some(&Type::F64) && ctx.operand_ty(&op) == Type::I64 {
                ctx.coerce_to_f64(op)
            } else {
                op
            }
        })
        .collect::<Vec<_>>();
    let cmp_ret = ctx.call_fn_value(cv.clone(), *ct, args, 0, 2);
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

/// User-comparator predicate wrapped in the §23.1.3.30.2 undefined
/// pre-probe: `pre_fid` (the Str-sentinel or Any-tag-5 probe) answers
/// `1`/`-1`/`0` (SortCompare result — an undefined side sorts last,
/// the comparator is NOT called) or `2` (no undefined — fall through
/// to the call).
fn emit_user_cmp_undef_pre(
    ctx: &mut LowerCtx<'_>,
    cv: &Operand,
    ct: &Type,
    prev: ValueId,
    cur: ValueId,
    pre_fid: crate::ssa::FuncId,
) -> ValueId {
    let slot = ctx.alloca_in_entry(Type::Bool, Some("__sort_pred"));
    let pre = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(pre_fid, vec![Operand::Value(prev), Operand::Value(cur)]),
        Type::I64,
        None,
    );
    let has_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(pre), Operand::ConstI64(2)),
        Type::Bool,
        None,
    );
    let pre_blk = ctx.f.add_block();
    let cb_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(has_undef),
            then_blk: pre_blk,
            else_blk: cb_blk,
        },
    );
    ctx.cur_block = pre_blk;
    let pre_pred = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sgt, Operand::Value(pre), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(pre_pred), Operand::Value(slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    ctx.cur_block = cb_blk;
    let cb_pred = emit_user_cmp_pred(ctx, cv, ct, prev, cur);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(cb_pred), Operand::Value(slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(after_blk));
    ctx.cur_block = after_blk;
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Bool, Operand::Value(slot), 0),
        Type::Bool,
        None,
    )
}
