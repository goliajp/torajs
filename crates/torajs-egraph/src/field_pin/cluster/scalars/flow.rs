//! Where a `pinned` scalar read stops naming what it read: the paths
//! from it that pass a write of its scalar, and the copy its later
//! uses read instead.

use crate::id_hash::HashSet;

use torajs_core::ssa::{
    BlockId, FuncId, Function, InstKind, Operand, PinWord, Terminator, ValueId,
    rewrite_value_operands, visit_value_operands,
};

/// Where `p` is defined: its block and index.
pub(super) fn def_of(f: &Function, p: ValueId) -> (usize, usize) {
    (f.blocks.iter().enumerate())
        .find_map(|(b, block)| {
            let i = block.insts.iter().position(|i| i.result == Some(p))?;
            Some((b, i))
        })
        .expect("a pinned value has a def")
}

/// Whether `kind` may write scalar `k`.
fn writes_scalar(kind: &InstKind, k: u8, writes: &HashSet<FuncId>) -> bool {
    match kind {
        InstKind::PinSet(PinWord::Scalar(j), _) => *j == k,
        InstKind::Call(t, _) => writes.contains(t),
        _ => false,
    }
}

/// Give the uses of `pinned` value `p` that some path from its def
/// reaches through a write of its scalar a copy made right after `p`.
/// The number of copies made, 0 or 1.
pub(super) fn copy_past_writes(f: &mut Function, p: ValueId, writes: &HashSet<FuncId>) -> u32 {
    let (d, i) = def_of(f, p);
    let InstKind::Pinned(PinWord::Scalar(k)) = f.blocks[d].insts[i].kind else {
        unreachable!("a scalar's pinned value");
    };
    let writes_at = |b: usize, j: usize| writes_scalar(&f.blocks[b].insts[j].kind, k, writes);
    // the first write in each block, and in `d` the first after `p`
    let first: Vec<Option<usize>> = (0..f.blocks.len())
        .map(|b| {
            let from = if b == d { i + 1 } else { 0 };
            (from..f.blocks[b].insts.len()).find(|&j| writes_at(b, j))
        })
        .collect();
    let clean_in = clean_on_entry(f, d, &first);
    // a write reads its operands before it writes, so a use in it is in time
    let late = |b: usize, j: usize| {
        let entered = b == d || clean_in[b];
        !entered || first[b].is_some_and(|w| j > w) || (b == d && j <= i)
    };
    let mut sites: Vec<(usize, usize)> = Vec::new();
    for (b, blk) in f.blocks.iter().enumerate() {
        for (j, x) in blk.insts.iter().enumerate() {
            let mut hit = false;
            visit_value_operands(&x.kind, |v| hit |= v == p);
            if hit && late(b, j) {
                sites.push((b, j));
            }
        }
        if term_reads(&blk.term, p) && late(b, blk.insts.len()) {
            sites.push((b, usize::MAX));
        }
    }
    if sites.is_empty() {
        return 0;
    }
    let ty = f.value_type(p);
    let c = f.insert_inst(
        BlockId(d as u32),
        i + 1,
        InstKind::Copy(ty, Operand::Value(p)),
        ty,
    );
    let to = |v: ValueId| if v == p { c } else { v };
    for (b, j) in sites {
        if j == usize::MAX {
            rewrite_term(&mut f.blocks[b].term, p, c);
        } else {
            // past the copy every index in block `d` moved by one
            let j = if b == d && j > i { j + 1 } else { j };
            rewrite_value_operands(&mut f.blocks[b].insts[j].kind, to);
        }
    }
    1
}

/// For each block, whether every path from the end of block `d` (a
/// `pinned` value's def, `first[d]` its first write after the def) to
/// it writes the scalar nowhere: false for a block a path from the
/// entry reaches without passing `d`, and for `d` itself.
fn clean_on_entry(f: &Function, d: usize, first: &[Option<usize>]) -> Vec<bool> {
    let n = f.blocks.len();
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (b, blk) in f.blocks.iter().enumerate() {
        for s in successors(&blk.term) {
            preds[s.0 as usize].push(b);
        }
    }
    let mut region = vec![false; n];
    let mut stack: Vec<usize> = successors(&f.blocks[d].term)
        .map(|s| s.0 as usize)
        .collect();
    while let Some(b) = stack.pop() {
        if !std::mem::replace(&mut region[b], true) {
            stack.extend(successors(&f.blocks[b].term).map(|s| s.0 as usize));
        }
    }
    let mut clean: Vec<bool> = (0..n).map(|b| region[b] && b != d).collect();
    let out = |clean: &[bool], b: usize| match b == d {
        true => first[d].is_none(),
        false => clean[b] && first[b].is_none(),
    };
    let mut changed = true;
    while changed {
        changed = false;
        for b in 0..n {
            if clean[b] && !(preds[b].iter()).all(|&q| (q == d || region[q]) && out(&clean, q)) {
                clean[b] = false;
                changed = true;
            }
        }
    }
    clean
}

fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> {
    let (a, b) = match term {
        Terminator::Br(t) => (Some(*t), None),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => (Some(*then_blk), Some(*else_blk)),
        Terminator::Ret(_) | Terminator::Unreachable => (None, None),
    };
    a.into_iter().chain(b)
}

fn term_reads(term: &Terminator, p: ValueId) -> bool {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) => *v == p,
        _ => false,
    }
}

fn rewrite_term(term: &mut Terminator, p: ValueId, c: ValueId) {
    if let Terminator::CondBr {
        cond: Operand::Value(v),
        ..
    }
    | Terminator::Ret(Some(Operand::Value(v))) = term
        && *v == p
    {
        *v = c;
    }
}
