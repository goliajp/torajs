//! The rest-slot half of the boxed adapter — collect
//! `argv[fixed..argc]` into an `Arr<Any>` for a rest parameter, and
//! convert it to the typed repr when the rest is typed.
//!
//! Moved verbatim out of `build.rs` (rotation 627) when that file
//! stood at 468 lines and the adapter build had to grow a receiver
//! lead for the argv-face static twin. The targets' reading of the
//! rest slot ([`rest_slot_kind`]) moved here from `targets.rs`.

use crate::ssa::{self, FuncId, InstKind, Operand, Terminator, Type};

use super::BoxedEntryIntrinsics;
use super::unbox::BoxedCoerceIntrinsics;

/// The rest-slot half of the adapter (刀 2 extraction, function-size
/// line): collect argv[fixed..argc] into an Arr<Any>, convert to the
/// typed repr when the rest is typed (NULL-gating the catchable
/// mismatch TypeError into an early undefined return), and record
/// the value's box as a post-call temp. Answers the slot operand and
/// the (possibly new) current block.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_rest_slot(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    coerce: &BoxedCoerceIntrinsics<'_>,
    arr_any_to_typed: FuncId,
    argv: ssa::ValueId,
    argc: ssa::ValueId,
    n_fixed: usize,
    arr_ty: Type,
    rest_kind: Option<i64>,
    obj_temps: &mut Vec<ssa::ValueId>,
) -> (ssa::ValueId, ssa::BlockId) {
    let mut entry = entry;
    let any_arr = emit_rest_collect(f, entry, intr, argv, argc, n_fixed, arr_ty);
    // A TYPED rest converts the Arr<Any> collection through the
    // assign-boundary kernel (per-slot coerce + kind stamp). A
    // mismatched element arms the catchable TypeError and answers
    // NULL; the gate below short-circuits before the body could
    // read the NULL block, and the pending throw surfaces at the
    // caller's check point.
    let arr = match rest_kind {
        Some(kind) => {
            let typed = f.append_inst(
                entry,
                InstKind::Call(
                    arr_any_to_typed,
                    vec![Operand::Value(any_arr), Operand::ConstI64(kind)],
                ),
                arr_ty,
                None,
            );
            // Release the Any collection now that the typed copy
            // owns the elements' new stakes.
            let boxed_src = f.append_inst(
                entry,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(4), Operand::Value(any_arr)],
                ),
                Type::Any,
                None,
            );
            f.append_void(
                entry,
                InstKind::Call(coerce.ids.anyv_rc_dec, vec![Operand::Value(boxed_src)]),
            );
            let pi = f.append_inst(
                entry,
                InstKind::PtrToInt(Operand::Value(typed)),
                Type::I64,
                None,
            );
            let isnull = f.append_inst(
                entry,
                InstKind::ICmp(ssa::IPred::Eq, Operand::Value(pi), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let throw_blk = f.add_block();
            let cont_blk = f.add_block();
            f.set_term(
                entry,
                Terminator::CondBr {
                    cond: Operand::Value(isnull),
                    then_blk: throw_blk,
                    else_blk: cont_blk,
                },
            );
            let undef = f.append_inst(
                throw_blk,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            f.set_term(throw_blk, Terminator::Ret(Some(Operand::Value(undef))));
            entry = cont_blk;
            typed
        }
        None => any_arr,
    };
    let boxed_arr = f.append_inst(
        entry,
        InstKind::Call(
            intr.any_box,
            vec![Operand::ConstI64(4), Operand::Value(arr)],
        ),
        Type::Any,
        None,
    );
    obj_temps.push(boxed_arr);
    (arr, entry)
}

fn emit_rest_collect(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    argv: ssa::ValueId,
    argc: ssa::ValueId,
    n_fixed: usize,
    arr_ty: Type,
) -> ssa::ValueId {
    let n_rest = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::Sub,
            Operand::Value(argc),
            Operand::ConstI64(n_fixed as i64),
        ),
        Type::I64,
        None,
    );
    let sign = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::AShr,
            Operand::Value(n_rest),
            Operand::ConstI64(63),
        ),
        Type::I64,
        None,
    );
    let not_sign = f.append_inst(
        entry,
        InstKind::BinOp(ssa::BinOp::Xor, Operand::Value(sign), Operand::ConstI64(-1)),
        Type::I64,
        None,
    );
    let count = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::And,
            Operand::Value(n_rest),
            Operand::Value(not_sign),
        ),
        Type::I64,
        None,
    );
    let arr = f.append_inst(
        entry,
        InstKind::Call(intr.arr_alloc_any, vec![Operand::Value(count)]),
        arr_ty,
        None,
    );
    let pi = f.append_inst(
        entry,
        InstKind::PtrToInt(Operand::Value(argv)),
        Type::I64,
        None,
    );
    let off = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::Add,
            Operand::Value(pi),
            Operand::ConstI64(8 * n_fixed as i64),
        ),
        Type::I64,
        None,
    );
    let pv = f.append_inst(
        entry,
        InstKind::IntToPtr(Operand::Value(off)),
        Type::Ptr,
        None,
    );
    f.append_void(
        entry,
        InstKind::Call(
            intr.arr_any_push,
            vec![
                Operand::Value(arr),
                Operand::Value(pv),
                Operand::Value(count),
                Operand::ConstPtrNull,
            ],
        ),
    );
    arr
}

/// The rest slot's shape for the adapter: `(is_rest, elem kind)`.
/// `None` refuses the target — a rest element the kernel cannot
/// stamp (a nested typed array) keeps the loud not-callable instead
/// of silent garbage. Split out of `targets::target_facts` (200-line fn
/// limit, rotation 650).
pub(super) fn rest_slot_kind(
    params: &[crate::ast::Param],
    user_tys: &[Type],
    arr_layouts: &crate::ssa::ArrLayouts,
) -> Option<(bool, Option<i64>)> {
    // Rest-param body: direct call sites pack trailing args into
    // an array literal (apply_rest_args), so the LAST param is an
    // ordinary Arr the static world always feeds. A runtime call
    // has no packing site — pre-fix the adapter unboxed argv
    // positionally and the rest param read one lone argument as
    // an array (garbage). The adapter collects argv[fixed..argc]
    // into a fresh Arr<Any> itself; a TYPED rest (刀 2, RFC
    // 20260815-fn-value-rest-spread) converts that collection
    // through `__torajs_arr_any_to_typed` with the element's
    // ARR_KIND. A REFCOUNTED element is one cell per slot, which
    // the kernel's HEAP path stakes — materializing a ShortStr
    // immediate rather than refusing one (the rotation-546 form).
    // Keeping this narrower than `arr_kind_chain_of`'s mapping is
    // what left `string[]` with no adapter at all, so
    // `const g = (...xs: string[]) => …; g('a')` died at "value
    // is not a function" (rotation 600).
    //
    // Nested typed arrays stay out for a reason still true: their
    // kind is a CHAIN and the kernel takes one level, so the inner
    // stamp would be lost. No adapter keeps such a call a loud
    // not-callable instead of silent garbage.
    let rest = params.last().is_some_and(|p| p.is_rest);
    let mut rest_kind: Option<i64> = None;
    let rest_elem = match user_tys.last() {
        Some(Type::Arr(id)) if rest => arr_layouts.get(id.0 as usize),
        _ => None,
    };
    // A rest whose SSA element is `Any` collects as `Arr<Any>`
    // exactly like an `any[]` annotation: the width analysis widens
    // a `number[]` rest to Any when its slot shares a class with an
    // any-escaped value (rotation 650 — a relay shell's `xs` unioned
    // with its target's, reached through one any-call site alongside
    // an arguments-face callee), and the HEAP arm below then refused
    // every integer box at `__torajs_arr_any_to_typed`'s pass 1.
    if rest
        && params.last().and_then(|p| p.type_ann.as_deref()) != Some("any[]")
        && rest_elem != Some(&Type::Any)
    {
        let elem = rest_elem;
        rest_kind = match elem {
            Some(Type::I64) | Some(Type::I32) => Some(torajs_rc::ARR_KIND_I64 as i64),
            Some(Type::F64) => Some(torajs_rc::ARR_KIND_F64 as i64),
            Some(Type::Bool) => Some(torajs_rc::ARR_KIND_BOOL as i64),
            Some(Type::Arr(_)) => None,
            Some(t) if t.is_refcounted() => Some(torajs_rc::ARR_KIND_HEAP as i64),
            _ => None,
        };
        rest_kind?;
    }
    Some((rest, rest_kind))
}
