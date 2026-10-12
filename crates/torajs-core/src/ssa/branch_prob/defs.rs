//! The defs the cold-arm rules read: each value's single def, or the
//! load a register cell copies.

use crate::ssa::{BinOp, Function, InstKind, Operand, ValueId};

/// Defining inst of every value written exactly once. φ destruction
/// gives a merged value one `copy` per predecessor; such values match
/// no rule, except a register cell every def of which copies a load of
/// the same word through the same pointer, or the same mask of one (a
/// header word kept in a register across a loop and read again after
/// the calls that may write it): it reads as that load or mask.
pub(super) fn single_defs(func: &Function) -> Defs<'_> {
    let (count, mut first) = count_defs(func);
    let single = |v: &ValueId| match count.get(v.0 as usize) {
        Some(1) => first[v.0 as usize],
        _ => None,
    };
    // a copied load, or a copied mask of one: (def, pointer, offset, mask)
    let loaded = |k: &InstKind| {
        let InstKind::Copy(_, Operand::Value(t)) = k else {
            return None;
        };
        match single(t)? {
            l @ InstKind::Load(_, ptr, off) => Some((l, *ptr, *off, None)),
            m @ InstKind::BinOp(BinOp::And, Operand::Value(w), Operand::ConstI64(mask)) => {
                match single(w)? {
                    InstKind::Load(_, ptr, off) => Some((m, *ptr, *off, Some(*mask))),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    // a several-def value reads as its first def's load or mask while
    // every later def, in block order, copies the same word
    let key = |k: &InstKind| loaded(k).map(|(_, p, o, m)| (p, o, m));
    let mut mixed = vec![false; count.len()];
    for (r, kind) in all_defs(func).filter(|(r, _)| count[r.0 as usize] > 1) {
        let (r, head) = (r.0 as usize, first[r.0 as usize].expect("a def was seen"));
        if !mixed[r] && !std::ptr::eq(kind, head) {
            mixed[r] = key(kind).is_none() || key(kind) != key(head);
        }
    }
    let cells: Vec<(usize, Option<&InstKind>)> = (0..count.len())
        .filter(|&v| count[v] > 1)
        .map(|v| {
            (
                v,
                first[v]
                    .filter(|_| !mixed[v])
                    .and_then(|h| Some(loaded(h)?.0)),
            )
        })
        .collect();
    for (v, def) in cells {
        first[v] = def;
    }
    Defs(first)
}

/// Every def in block order: its result and its inst.
fn all_defs(func: &Function) -> impl Iterator<Item = (ValueId, &InstKind)> {
    (func.blocks.iter().flat_map(|b| &b.insts)).filter_map(|i| Some((i.result?, &i.kind)))
}

/// Per value index, its def count and first def.
fn count_defs(func: &Function) -> (Vec<u32>, Vec<Option<&InstKind>>) {
    let mut count = vec![0u32; func.values.len()];
    let mut first: Vec<Option<&InstKind>> = vec![None; func.values.len()];
    for (r, kind) in all_defs(func) {
        // a hand-built function may leave `values` short
        if count.len() <= r.0 as usize {
            count.resize(r.0 as usize + 1, 0);
            first.resize(r.0 as usize + 1, None);
        }
        count[r.0 as usize] += 1;
        first[r.0 as usize].get_or_insert(kind);
    }
    (count, first)
}

/// The single def of each value (or the load a register cell copies),
/// by value index.
pub(super) struct Defs<'a>(pub(super) Vec<Option<&'a InstKind>>);

pub(super) fn value_def<'a>(op: &Operand, defs: &Defs<'a>) -> Option<&'a InstKind> {
    match op {
        Operand::Value(v) => defs.0.get(v.0 as usize).copied().flatten(),
        _ => None,
    }
}
