//! What the header passes (`load_licm`, `len_promote`) admit on top of
//! the element analysis' escape discipline.
//!
//! An array literal the escape verifier keeps in the frame is born as
//! an `alloca_bytes` cell, not through `__torajs_arr_alloc`: lowering
//! writes its header word, props slot and data pointer itself, the
//! data pointer aiming at the cell's own inline slots (`cell + 40`).
//! Those birth stores, and that address, are the only uses this module
//! adds for such a cell; everything else it meets goes through the
//! same discipline as a heap array.
//!
//! The typed element store's cold arms hand the array to three
//! kernels. None of them keeps the pointer or frees the cell, but each
//! may rewrite the header: the grow writes length, capacity, head and
//! data pointer, the refusal and the revive may grow the props bag and
//! the index flags. They are admitted only here, where a header word
//! read after one of them is read again (`load_licm::reread`); the
//! element analysis still drops an array that reaches them, as it
//! cannot bound the value the grow writes.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Type, ValueId};

use super::elem::{name_of, val};
use super::elem_kernels::{ARR_DATA_PTR_OFF, ARR_HEADER_WORD_OFF};

/// The header's size; a frame cell's inline slots start here.
pub(crate) const ARR_CELL_SIZE: u64 = 40;
/// The props-bag slot.
pub(crate) const ARR_PROPS_OFF: u64 = 24;

/// The typed element store's cold-arm kernels (see the module doc).
pub(crate) const HEADER_WRITE_CALLS: &[&str] = &[
    "__torajs_arr_typed_set_grow",
    "__torajs_arr_index_check_store",
    "__torajs_arr_index_revive_idx",
];

/// Frame-cell array literals: a single `alloca_bytes` def typed as an
/// array, large enough for the header.
pub(super) fn frame_cell_roots(
    func: &Function,
    defs: &HashMap<ValueId, Vec<&InstKind>>,
) -> HashSet<ValueId> {
    defs.iter()
        .filter(|(v, ds)| {
            matches!(ds.as_slice(), [InstKind::AllocaBytes(n)] if *n >= ARR_CELL_SIZE)
                && matches!(func.value_type(**v), Type::Arr(_))
        })
        .map(|(v, _)| *v)
        .collect()
}

/// `cell + 40` of a frame cell: its inline data pointer, as (value,
/// cell) pairs to seed the data-pointer map with.
pub(super) fn inline_data_ptrs(
    defs: &HashMap<ValueId, Vec<&InstKind>>,
    stack: &HashSet<ValueId>,
) -> Vec<(ValueId, ValueId)> {
    let mut out: Vec<(ValueId, ValueId)> = defs
        .iter()
        .filter_map(|(v, ds)| match ds.as_slice() {
            [InstKind::BinOp(BinOp::Add, Operand::Value(c), Operand::ConstI64(k))]
                if *k as u64 == ARR_CELL_SIZE && stack.contains(c) =>
            {
                Some((*v, *c))
            }
            _ => None,
        })
        .collect();
    out.sort_by_key(|(v, _)| v.0);
    out
}

/// Whether `kind` is one of the uses this module adds: a frame cell's
/// birth stores and inline address, or a header kernel handed a
/// tracked array and nothing else tracked.
pub(super) fn admitted(
    result: Option<ValueId>,
    kind: &InstKind,
    class: &HashMap<ValueId, ValueId>,
    base: &HashMap<ValueId, ValueId>,
    stack: &HashSet<ValueId>,
    names: &[String],
) -> bool {
    let frame = |p: &Operand| {
        val(p)
            .and_then(|p| class.get(&p).copied())
            .filter(|r| stack.contains(r))
    };
    let tracked =
        |op: &Operand| val(op).is_some_and(|v| class.contains_key(&v) || base.contains_key(&v));
    match kind {
        InstKind::Store(v, p, off) => frame(p).is_some_and(|rep| match *off {
            ARR_HEADER_WORD_OFF | ARR_PROPS_OFF => {
                matches!(v, Operand::ConstI64(_) | Operand::ConstPtrNull)
            }
            ARR_DATA_PTR_OFF => val(v).and_then(|b| base.get(&b)) == Some(&rep),
            _ => false,
        }),
        InstKind::BinOp(BinOp::Add, p, Operand::ConstI64(k)) if *k as u64 == ARR_CELL_SIZE => {
            frame(p).is_some_and(|rep| result.and_then(|r| base.get(&r)) == Some(&rep))
        }
        InstKind::Call(f, args) => {
            HEADER_WRITE_CALLS.contains(&name_of(names, *f))
                && args
                    .first()
                    .and_then(val)
                    .is_some_and(|a| class.contains_key(&a))
                && !args[1..].iter().any(tracked)
        }
        _ => false,
    }
}
