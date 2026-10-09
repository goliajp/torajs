//! The length of a concat nobody reads: `(a + b).length` is
//! `a.length + b.length` (ECMA-262 §6.1.4 — a String is a sequence of
//! code units, concatenation joins the sequences), so a concat whose
//! result is only measured and dropped never has to be built.
//!
//! ```text
//! %r = call __torajs_str_concat(%a, %b)       head: la = len a; lb = len b
//! %l = load i32, %r +8                              sum = la + lb
//! %z = zext_i32 %l                                  cond_br sum > MAX | a null | b null, slow, fast
//! call __torajs_str_drop(%r)                  fast: %n = copy sum          → join
//!                                             slow: concat; %n = len; drop → join
//!                                             join: %l = copy %n; %z = copy %l
//! ```
//!
//! A cell is measured only when every read of it is a length load
//! whose readers all widen it, a `str_drop`, or a `copy` into another
//! such cell, and every def of it is a concat or a `copy` from one:
//! the group closed under those copies is rewritten together, each
//! copy of a cell becoming a copy of its length. That is the shape
//! the loop versions leave — an integer loop's overflow bail hands
//! its concat to the float loop's cell, which drops it. The drops may
//! sit anywhere after the concat (a loop drops at its latch, one per
//! exit); the slow arm drops right after the kernel instead, which
//! nothing can tell apart since nothing reads the cell. Each length
//! load becomes a copy of the cell where it stood, so a cell redefined
//! between a load and its widening still measures what the load saw.
//!
//! The engines reach the same answer through ropes: a concat is a
//! node holding two lengths until someone reads the characters.
//!
//! The slow arm keeps the kernel for the two cases its fast path does
//! not spell, and measures what the kernel answered: past
//! [`STR_MAX_LEN`] it raises a RangeError and answers its left operand,
//! which a `catch` reading the result sees; and a NULL in a Str-typed
//! operand reads as the text "null" (the kernel's own arm), which the
//! head cannot measure by loading. Lengths are each at most `MAX`, so
//! the i64 sum cannot wrap.
//!
//! Before concat_num_fuse and str_append, which would otherwise claim
//! the concat for an append. `TORAJS_CONCAT_LEN_OFF=1` skips,
//! `TORAJS_CONCAT_LEN_STATS=1` counts.

mod diamond;
#[cfg(test)]
mod tests;

use crate::float_demote::push_value;
use crate::id_hash::{HashMap, HashSet};
use crate::str_append::kernels::{Kernels, Kind};

use torajs_core::ssa::{
    BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

/// Mirrors `torajs_str::max_len::STR_MAX_LEN` by name, the way the
/// runtime crates mirror the layout offsets: 2^31 - 1 code units.
const STR_MAX_LEN: i64 = (1 << 31) - 1;
/// Offset of the u32 length in a Str header and of the i64 length in
/// a Substr view, as `ssa_lower_member` reads them.
const STR_LEN_OFF: u64 = 8;
const SUBSTR_LEN_OFF: u64 = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ConcatLenStats {
    /// Concats replaced by the sum of their operands' lengths.
    pub folded: u32,
}

pub fn fold_concat_lens(module: &mut Module) -> ConcatLenStats {
    let mut stats = ConcatLenStats::default();
    let Some(kernels) = Kernels::find(module) else {
        return stats;
    };
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let candidates = measured_concats(func, &kernels);
        if candidates.is_empty() {
            continue;
        }
        let mut seen: HashSet<ValueId> = HashSet::default();
        let mut index = Index::of(func);
        for r in candidates {
            if seen.contains(&r) {
                continue;
            }
            seen.insert(r);
            let Some(group) = measured_group(func, &index, &kernels, r) else {
                continue;
            };
            seen.extend(group.members.iter().copied());
            stats.folded += fold_group(func, &kernels, &group);
            index = Index::of(func);
        }
    }
    stats
}

/// The results of borrowing concat calls that some `load i32 +8`
/// reads, in definition order: one linear scan, so a function with
/// none (most of them) costs nothing more.
fn measured_concats(func: &Function, kernels: &Kernels) -> Vec<ValueId> {
    let mut concats = Vec::new();
    let mut measured: HashSet<ValueId> = HashSet::default();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        match &inst.kind {
            InstKind::Load(Type::I32, Operand::Value(r), STR_LEN_OFF) => {
                measured.insert(*r);
            }
            InstKind::Call(..) => {
                if let Some(r) = concat_of(inst, kernels).and(inst.result) {
                    concats.push(r);
                }
            }
            _ => {}
        }
    }
    concats.retain(|r| measured.contains(r));
    concats
}

/// Where each value is defined and read, built in one walk.
struct Index {
    defs: HashMap<ValueId, Vec<(usize, usize)>>,
    uses: HashMap<ValueId, Vec<(usize, usize)>>,
    /// Values a terminator reads.
    term_uses: HashSet<ValueId>,
}

impl Index {
    fn of(func: &Function) -> Self {
        let mut index = Index {
            defs: HashMap::default(),
            uses: HashMap::default(),
            term_uses: HashSet::default(),
        };
        for (bi, block) in func.blocks.iter().enumerate() {
            for (i, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    index.defs.entry(r).or_default().push((bi, i));
                }
                visit_value_operands(&inst.kind, |v| {
                    index.uses.entry(v).or_default().push((bi, i))
                });
            }
            if let Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) = block.term
            {
                index.term_uses.insert(v);
            }
        }
        index
    }

    fn defs(&self, v: ValueId) -> &[(usize, usize)] {
        self.defs.get(&v).map(Vec::as_slice).unwrap_or(&[])
    }

    fn uses(&self, v: ValueId) -> &[(usize, usize)] {
        self.uses.get(&v).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn at(func: &Function, (bi, i): (usize, usize)) -> &Inst {
    &func.blocks[bi].insts[i]
}

/// The operands of `inst` when it calls a borrowing concat kernel.
fn concat_of(inst: &Inst, kernels: &Kernels) -> Option<((Operand, Kind), (Operand, Kind))> {
    let (k, args) = kernels.call(inst)?;
    (!k.consumes_left).then(|| ((args[0], k.left), (args[1], k.right)))
}

/// The cells that only measure each other's concats.
struct Group {
    members: HashSet<ValueId>,
    /// Each `load i32 +8` result and the member it measures.
    lens: HashMap<ValueId, ValueId>,
}

/// The group of `r`: cells reached from it through `copy`, each
/// defined only by concats and copies from members, read only by
/// length loads whose readers widen them, drops and copies into
/// members.
fn measured_group(func: &Function, index: &Index, kernels: &Kernels, r: ValueId) -> Option<Group> {
    let mut members: HashSet<ValueId> = HashSet::default();
    let mut todo = vec![r];
    let mut lens = HashMap::default();
    while let Some(m) = todo.pop() {
        if !members.insert(m) {
            continue;
        }
        if index.term_uses.contains(&m) || index.defs(m).is_empty() {
            return None;
        }
        for &pos in index.defs(m) {
            let inst = at(func, pos);
            match &inst.kind {
                _ if concat_of(inst, kernels).is_some() => {}
                InstKind::Copy(Type::Str, Operand::Value(u)) => todo.push(*u),
                _ => return None,
            }
        }
        for &pos in index.uses(m) {
            let inst = at(func, pos);
            match (&inst.kind, inst.result) {
                (InstKind::Load(Type::I32, _, STR_LEN_OFF), Some(l)) => {
                    if !widened_only(func, index, l) {
                        return None;
                    }
                    lens.insert(l, m);
                }
                (InstKind::Call(fid, _), None) if *fid == kernels.str_drop => {}
                (InstKind::Copy(Type::Str, _), Some(v)) => todo.push(v),
                _ => return None,
            }
        }
    }
    (!lens.is_empty()).then_some(Group { members, lens })
}

/// `l` has one def and every reader is a `zext_i32` of it.
fn widened_only(func: &Function, index: &Index, l: ValueId) -> bool {
    index.defs(l).len() == 1
        && !index.term_uses.contains(&l)
        && index
            .uses(l)
            .iter()
            .all(|&pos| matches!(at(func, pos).kind, InstKind::ZExtI32ToI64(_)))
}

/// Every member's length as a fresh i64 cell: the drops go, each load
/// and each widening of one becomes a copy of the cell, copies
/// between members copy the cells, and each concat def becomes the
/// diamond.
fn fold_group(func: &mut Function, kernels: &Kernels, group: &Group) -> u32 {
    let n: HashMap<ValueId, ValueId> = group
        .members
        .iter()
        .map(|&m| (m, push_value(&mut func.values, Type::I64)))
        .collect();
    for &l in group.lens.keys() {
        func.values[l.0 as usize].ty = Type::I64;
    }
    for block in func.blocks.iter_mut() {
        block.insts.retain(
            |inst| !matches!(kernels.dropped(inst), Some(Operand::Value(m)) if n.contains_key(m)),
        );
        for inst in block.insts.iter_mut() {
            match (inst.result, &inst.kind) {
                (Some(l), InstKind::Load(Type::I32, _, STR_LEN_OFF))
                    if group.lens.contains_key(&l) =>
                {
                    inst.kind = InstKind::Copy(Type::I64, Operand::Value(n[&group.lens[&l]]));
                }
                (_, InstKind::ZExtI32ToI64(Operand::Value(l))) if group.lens.contains_key(l) => {
                    inst.kind = InstKind::Copy(Type::I64, Operand::Value(*l));
                }
                (Some(m), InstKind::Copy(Type::Str, Operand::Value(u)))
                    if n.contains_key(&m) && n.contains_key(u) =>
                {
                    inst.kind = InstKind::Copy(Type::I64, Operand::Value(n[u]));
                    inst.result = Some(n[&m]);
                }
                _ => {}
            }
        }
    }
    let mut folded = 0;
    // each split appends its slow arm after every earlier one, so the
    // scan stops short of the arms it made
    let mut bi = 0;
    while bi + folded < func.blocks.len() {
        let site = func.blocks[bi]
            .insts
            .iter()
            .enumerate()
            .find_map(|(at, inst)| {
                let r = inst.result.filter(|r| n.contains_key(r))?;
                concat_of(inst, kernels).map(|(left, right)| (at, n[&r], left, right))
            });
        match site {
            Some((at, cell, left, right)) => {
                diamond::fold_site(func, kernels, BlockId(bi as u32), at, cell, left, right);
                folded += 1;
            }
            None => bi += 1,
        }
    }
    folded as u32
}
