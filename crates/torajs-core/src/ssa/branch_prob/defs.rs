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
    let DefCounts {
        count,
        mut first,
        later,
    } = count_defs(func);
    if later.is_empty() {
        return Defs(first);
    }
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
    for &(r, kind) in &later {
        let (r, k) = (r as usize, key(kind));
        mixed[r] |= k.is_none() || k != key(first[r].expect("a def was seen"));
    }
    let cells: Vec<(usize, Option<&InstKind>)> = (later.iter())
        .map(|&(v, _)| {
            let v = v as usize;
            let def = (first[v].filter(|_| !mixed[v])).and_then(|h| Some(loaded(h)?.0));
            (v, def)
        })
        .collect();
    for (v, def) in cells {
        first[v] = def;
    }
    Defs(first)
}

/// What one walk over the defs reads.
struct DefCounts<'a> {
    /// Per value index, its number of defs.
    count: Vec<u32>,
    /// Per value index, its first def.
    first: Vec<Option<&'a InstKind>>,
    /// Every def of a value after its first, in block order.
    later: Vec<(u32, &'a InstKind)>,
}

fn count_defs(func: &Function) -> DefCounts<'_> {
    let mut count = vec![0u32; func.values.len()];
    let mut first: Vec<Option<&InstKind>> = vec![None; func.values.len()];
    let mut later = Vec::new();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        let Some(r) = inst.result else {
            continue;
        };
        let r_ = r.0 as usize;
        // a hand-built function may leave `values` short
        if count.len() <= r_ {
            count.resize(r_ + 1, 0);
            first.resize(r_ + 1, None);
        }
        count[r_] += 1;
        match first[r_] {
            None => first[r_] = Some(&inst.kind),
            Some(_) => later.push((r.0, &inst.kind)),
        }
    }
    DefCounts {
        count,
        first,
        later,
    }
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
