//! What an async body's return hands its result promise: the value
//! form it settles the result in, and the element kind of an array it
//! returns.

use crate::ssa::{self, FuncId, Inst, InstKind, Operand, Type};

/// Each array the body returns marked with its element kind, as
/// `Promise.resolve` marks the array it settles with: the result is
/// read in the `any` lane too (a `.then` handler taking `any`), which
/// reads the elements by the mark.
pub(super) fn mark_returned_arrays(
    f: &mut ssa::Function,
    arr_layouts: &ssa::ArrLayouts,
    mark: FuncId,
) {
    for b in &mut f.blocks {
        let ssa::Terminator::Ret(Some(Operand::Value(v))) = b.term else {
            continue;
        };
        let Type::Arr(id) = f.values[v.0 as usize].ty else {
            continue;
        };
        let elem = arr_layouts[id.0 as usize];
        let chain = crate::ssa_lower_arr_kind_mark::arr_kind_chain_of(arr_layouts, &elem, 0);
        if chain != 0 {
            let args = vec![Operand::Value(v), Operand::ConstI64(chain as i64)];
            b.insts.push(Inst {
                result: None,
                kind: InstKind::Call(mark, args),
                origin: None,
            });
        }
    }
}

/// The value form a body returning `ty` settles its result with
/// (`torajs-promise` `REPR_*`): the form `Promise.resolve` of a value
/// of that type stamps, a number at the width the body returns it
/// (the width of the promise's value point, which every `await` of a
/// call to the body reads).
pub(super) fn ret_repr(ty: Type) -> i64 {
    use crate::ssa_lower_promise_repr_mark as m;
    match m::promise_value_repr(&ty, false, false) {
        Some(r) => r,
        None if ty.is_refcounted() => m::REPR_HEAP,
        None => m::REPR_I64,
    }
}
