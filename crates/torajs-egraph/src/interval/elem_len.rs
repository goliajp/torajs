//! Length cells — the element analysis's admission discipline applied
//! to one more field of a tracked array: its length.
//!
//! A tracked allocation's address never leaves the function's SSA, so
//! its length field is written only where this module can see it: the
//! allocation itself (`__torajs_arr_alloc*` starts every array at
//! length 0), a `Store` at `ARR_LEN_OFF` on an alias of the array, and
//! any admitted kernel call on it that is not on the length-inert list
//! (a push bumps the length; a drop ends it). Those are the cell's defs,
//! each at its program point; a `Load(I64, alias, ARR_LEN_OFF)` is a
//! read. The point evaluator resolves a read to its unique last def
//! exactly as it does for an SSA cell, which is how a loop bounded by
//! `i < xs.length` after a counted push loop learns its trip count.

use crate::id_hash::HashMap;

use torajs_core::ssa::{Function, InstKind, Operand, Type, ValueId};

use super::elem;
use super::elem_kernels::*;

/// A length def and its `(block index, inst index)` position
pub(crate) type LenDefAt = ((usize, usize), LenDef);

/// What one def leaves in the length field.
pub(crate) enum LenDef {
    /// The operand's value as read at the def's point.
    Known(Operand),
    /// A kernel moved the length by an amount this module does not model.
    Opaque,
}

/// The length cells of one function, keyed by allocation root.
#[derive(Default)]
pub(crate) struct LenCells {
    /// Root → its length defs at `(block index, inst index)`.
    pub(crate) defs: HashMap<ValueId, Vec<LenDefAt>>,
    /// Length-load result → root.
    pub(crate) reads: HashMap<ValueId, ValueId>,
}

/// Whether `kind` reads an array's length field (what `len_cells`
/// records a read for, when the pointer is a tracked array).
pub(crate) fn is_len_load(kind: &InstKind) -> bool {
    matches!(kind, InstKind::Load(Type::I64, _, ARR_LEN_OFF))
}

/// The length cells of every fully tracked allocation in `func`; empty
/// when `callee_names` is empty (no call can be recognized).
pub(crate) fn len_cells(func: &Function, callee_names: &[String]) -> LenCells {
    let mut cells = LenCells::default();
    let Some(a) = elem::analyze(func, callee_names, false) else {
        return cells;
    };
    let live = &a.classes.live;
    let class = &a.classes.class;
    let rep_of = |op: &Operand| match op {
        Operand::Value(v) => class.get(v).copied().filter(|r| live.contains(r)),
        _ => None,
    };
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            let at = (bi, ii);
            match &inst.kind {
                InstKind::Call(f, args) => {
                    let n = callee_names
                        .get(f.0 as usize)
                        .map(String::as_str)
                        .unwrap_or("");
                    if ALLOC_CALLS.contains(&n) {
                        if let Some(r) = inst.result.filter(|r| live.contains(r)) {
                            cells
                                .defs
                                .entry(r)
                                .or_default()
                                .push((at, LenDef::Known(Operand::ConstI64(0))));
                        }
                    } else if let Some(rep) = args.first().and_then(rep_of)
                        && !LEN_INERT_CALLS.contains(&n)
                    {
                        cells
                            .defs
                            .entry(rep)
                            .or_default()
                            .push((at, LenDef::Opaque));
                    }
                }
                InstKind::Store(value, ptr, ARR_LEN_OFF) => {
                    if let Some(rep) = rep_of(ptr) {
                        cells
                            .defs
                            .entry(rep)
                            .or_default()
                            .push((at, LenDef::Known(*value)));
                    }
                }
                InstKind::Load(Type::I64, ptr, ARR_LEN_OFF) => {
                    if let (Some(rep), Some(r)) = (rep_of(ptr), inst.result) {
                        cells.reads.insert(r, rep);
                    }
                }
                _ => {}
            }
        }
    }
    cells
}
