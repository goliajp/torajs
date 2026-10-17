//! Which instructions make a second name for a tracked array or for its
//! data pointer. Split from `elem.rs` at the file-size line.

use crate::id_hash::HashMap;

use torajs_core::ssa::{BinOp, InstKind, Operand, ValueId};

use super::elem::{name_of, val};
use super::elem_kernels::*;

pub(super) enum AliasKind {
    /// Same thing as the source operand.
    Same,
    /// The data pointer of the source array.
    DataPtr,
    /// A data pointer moved forward within its own buffer (the fill
    /// loop's slot[0] address). Only a data pointer qualifies, and only
    /// by a non-negative constant or a whole number of slots (`x << 3`):
    /// a write through the result must still land among the slots.
    Advanced,
}

pub(super) fn alias_source(
    kind: &InstKind,
    names: &[String],
    defs: &HashMap<ValueId, Vec<&InstKind>>,
) -> Option<(ValueId, AliasKind)> {
    let whole_slots = |off: &Operand| match off {
        Operand::ConstI64(k) => *k >= 0,
        Operand::Value(o) => matches!(
            defs.get(o).map(Vec::as_slice),
            Some([InstKind::BinOp(BinOp::Shl, _, Operand::ConstI64(3))])
        ),
        _ => false,
    };
    match kind {
        InstKind::Copy(_, op) | InstKind::Identity(op) => val(op).map(|v| (v, AliasKind::Same)),
        InstKind::Load(_, op, ARR_DATA_PTR_OFF) => val(op).map(|v| (v, AliasKind::DataPtr)),
        InstKind::BinOp(BinOp::Add, op, off) if whole_slots(off) => {
            val(op).map(|v| (v, AliasKind::Advanced))
        }
        InstKind::Call(f, args) => {
            let n = name_of(names, *f);
            if IDENTITY_CALLS.contains(&n) || IDENTITY_PUSH_CALLS.contains(&n) {
                args.first().and_then(val).map(|v| (v, AliasKind::Same))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Optimistic membership: which values are aliases of a root, and
/// which are data pointers loaded out of one. Both are confirmed by
/// `elem::scan_inst`, which kills a class whose alias-producing use did
/// not land back in the same class.
pub(super) fn classify_values(
    defs: &HashMap<ValueId, Vec<&InstKind>>,
    roots: &[ValueId],
    names: &[String],
    seed_base: &[(ValueId, ValueId)],
) -> (HashMap<ValueId, ValueId>, HashMap<ValueId, ValueId>) {
    let mut class: HashMap<ValueId, ValueId> = roots.iter().map(|r| (*r, *r)).collect();
    let mut base: HashMap<ValueId, ValueId> = seed_base.iter().copied().collect();
    let mut ordered: Vec<ValueId> = defs.keys().copied().collect();
    ordered.sort_by_key(|v| v.0);
    let mut moving = true;
    while moving {
        moving = false;
        for v in &ordered {
            if class.contains_key(v) || base.contains_key(v) {
                continue;
            }
            // Every definition has to name the same class in the same
            // role, or the value may also hold an array this analysis
            // never saw.
            let mut agreed: Option<(ValueId, bool)> = None;
            let mut ok = true;
            for d in &defs[v] {
                let seen = match alias_source(d, names, defs) {
                    // A copy of a data pointer stays a data pointer;
                    // a copy of the array stays the array.
                    Some((src, AliasKind::Same)) => class
                        .get(&src)
                        .map(|r| (*r, false))
                        .or_else(|| base.get(&src).map(|r| (*r, true))),
                    Some((src, AliasKind::DataPtr)) => class.get(&src).map(|r| (*r, true)),
                    Some((src, AliasKind::Advanced)) => base.get(&src).map(|r| (*r, true)),
                    None => None,
                };
                match (seen, agreed) {
                    (Some(x), None) => agreed = Some(x),
                    (Some(x), Some(y)) if x == y => {}
                    _ => ok = false,
                }
                if !ok {
                    break;
                }
            }
            let Some((rep, is_base)) = agreed.filter(|_| ok) else {
                continue;
            };
            if is_base { &mut base } else { &mut class }.insert(*v, rep);
            moving = true;
        }
    }
    (class, base)
}
