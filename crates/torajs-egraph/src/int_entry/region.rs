//! The fast copy of a loop about to be versioned: every block under
//! a fresh id, and every value the loop defines under a fresh id —
//! its multi-def cells too. The loop as it was stays the slow copy.
//!
//! The cells are what make the copy worth having. A cell both copies
//! wrote would carry, as its one flow-insensitive fact, the join of
//! what either may store, and the slow copy bounds nothing; and a set
//! of f64 cells two loops touch is not a region float demotion can
//! close. So the fast copy's cells are its own, seeded on the way in
//! from what the loop's cells were last given.
//!
//! On the way out neither copy may write the other's cells, for the
//! same reason. What the code after the loop read from a loop cell it
//! reads from an out cell instead, which every exit of either copy
//! stores its own cell into.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{Block, BlockId, Function, Inst, InstKind, Operand, Terminator, ValueId};

use crate::float_demote::cfg::{
    remap_term_operand, retarget, retarget_edge, successors, term_operand_value,
};
use crate::float_demote::push_value;
use crate::rc_peephole::visit_value_operands;
use crate::slot_forward::rewrite_operands;

pub(crate) struct FastCopy {
    /// loop block → its copy
    pub(crate) blocks: HashMap<BlockId, BlockId>,
    /// value the loop defines → the copy's
    pub(crate) values: HashMap<ValueId, ValueId>,
}

impl FastCopy {
    pub(super) fn operand(&self, op: Operand) -> Operand {
        match op {
            Operand::Value(v) => Operand::Value(self.values.get(&v).copied().unwrap_or(v)),
            other => other,
        }
    }
}

/// A single-def value the region defines and something outside it
/// reads: only one of the two copies defines it under that id.
pub(super) fn escapes(
    func: &Function,
    region: &HashSet<BlockId>,
    cells: &HashSet<ValueId>,
) -> bool {
    let mut own: HashSet<ValueId> = HashSet::default();
    for b in region {
        for inst in &func.blocks[b.0 as usize].insts {
            own.extend(inst.result.filter(|r| !cells.contains(r)));
        }
    }
    func.blocks
        .iter()
        .filter(|b| !region.contains(&b.id))
        .any(|b| !reads(b, &own).is_empty())
}

/// The members of `of` the block reads.
fn reads(block: &Block, of: &HashSet<ValueId>) -> HashSet<ValueId> {
    let mut read: HashSet<ValueId> = term_operand_value(&block.term)
        .filter(|v| of.contains(v))
        .into_iter()
        .collect();
    for inst in &block.insts {
        visit_value_operands(&inst.kind, |v| {
            if of.contains(&v) {
                read.insert(v);
            }
        });
    }
    read
}

/// The loop cells the code after the loop reads, and the blocks that
/// read them — `None` when some reader cannot be told apart from one
/// that runs before the loop.
///
/// A block the header dominates runs only after the loop was entered,
/// and nothing between an exit and it runs without the header having
/// dominated that too; so as long as no such block stores the cell,
/// what it reads is what the last exit left. A reader the header does
/// not dominate, yet an exit reaches — the loop sat under a branch,
/// or inside another loop that carries the cell round — may be
/// reading either.
pub(crate) fn readers_after(
    func: &Function,
    region: &[BlockId],
    header: BlockId,
    dom: &DominatorTree,
    cells: &HashSet<ValueId>,
) -> Option<(Vec<ValueId>, Vec<BlockId>)> {
    let in_region: HashSet<BlockId> = region.iter().copied().collect();
    let own: HashSet<ValueId> = cells_of(func, region, cells).into_iter().collect();
    // what the exits reach without going back through the loop
    let mut reached: HashSet<BlockId> = HashSet::default();
    let mut work: Vec<BlockId> = region
        .iter()
        .flat_map(|b| successors(&func.blocks[b.0 as usize].term))
        .filter(|s| !in_region.contains(s))
        .collect();
    while let Some(b) = work.pop() {
        if in_region.contains(&b) || !reached.insert(b) {
            continue;
        }
        work.extend(successors(&func.blocks[b.0 as usize].term));
    }
    let mut out: HashSet<ValueId> = HashSet::default();
    let mut readers = Vec::new();
    for block in func.blocks.iter().filter(|b| reached.contains(&b.id)) {
        let after = dom.dominates(header, block.id);
        let read = reads(block, &own);
        if !read.is_empty() {
            if !after {
                return None;
            }
            out.extend(read);
            readers.push(block.id);
        }
        let stores = block
            .insts
            .iter()
            .any(|i| i.result.is_some_and(|r| own.contains(&r)));
        if stores && after {
            return None;
        }
    }
    let mut out: Vec<ValueId> = out.into_iter().collect();
    out.sort_by_key(|v| v.0);
    Some((out, readers))
}

/// Append the copy. Nothing enters it yet.
pub(crate) fn clone_blocks(func: &mut Function, region: &[BlockId]) -> FastCopy {
    let mut op_map: HashMap<ValueId, Operand> = HashMap::default();
    let mut values: HashMap<ValueId, ValueId> = HashMap::default();
    for b in region {
        for i in 0..func.blocks[b.0 as usize].insts.len() {
            let Some(r) = func.blocks[b.0 as usize].insts[i].result else {
                continue;
            };
            if values.contains_key(&r) {
                continue;
            }
            let ty = func.values[r.0 as usize].ty;
            let fresh = push_value(&mut func.values, ty);
            values.insert(r, fresh);
            op_map.insert(r, Operand::Value(fresh));
        }
    }
    let base = func.blocks.len() as u32;
    let blocks: HashMap<BlockId, BlockId> = region
        .iter()
        .enumerate()
        .map(|(i, b)| (*b, BlockId(base + i as u32)))
        .collect();
    for b in region {
        let src = func.blocks[b.0 as usize].clone();
        let insts = src
            .insts
            .into_iter()
            .map(|mut inst| {
                inst.result = inst.result.map(|r| values[&r]);
                rewrite_operands(&mut inst.kind, &op_map);
                inst
            })
            .collect();
        func.blocks.push(Block {
            id: blocks[b],
            insts,
            term: retarget(remap_term_operand(src.term, &op_map), &blocks),
        });
    }
    FastCopy { blocks, values }
}

/// The loop's cells, in id order.
pub(crate) fn cells_of(
    func: &Function,
    region: &[BlockId],
    cells: &HashSet<ValueId>,
) -> Vec<ValueId> {
    let mut own: Vec<ValueId> = region
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .filter_map(|inst| inst.result.filter(|r| cells.contains(r)))
        .collect();
    own.sort_by_key(|v| v.0);
    own.dedup();
    own
}

/// What one entering block last stored into `cell`, when it stored it by
/// a `copy` whose operand still holds that value at the block's end.
fn stored(
    func: &Function,
    block: usize,
    cell: ValueId,
    cells: &HashSet<ValueId>,
) -> Option<Operand> {
    let insts = &func.blocks[block].insts;
    let at = insts.iter().rposition(|i| i.result == Some(cell))?;
    let InstKind::Copy(_, from) = &insts[at].kind else {
        return None;
    };
    let kept = match from {
        Operand::Value(v) if cells.contains(v) => {
            !insts[at + 1..].iter().any(|i| i.result == Some(*v))
        }
        _ => true,
    };
    kept.then_some(*from)
}

/// Give each of the copy's cells its value at the loop's entry, in
/// the block the checks fall through into.
///
/// Where every block that enters the loop stored the cell itself, and
/// the same thing — `i = 0` in the preheader — the copy's cell takes
/// that operand. It could as well read the loop's cell, and does when
/// an entering block left the cell as it found it; but that read
/// answers with everything the cell may ever hold, the slow loop's
/// unbounded counter included.
pub(super) fn seed_cells(
    func: &mut Function,
    region: &[BlockId],
    header: BlockId,
    outside: usize,
    pass: BlockId,
    cells: &HashSet<ValueId>,
    copy: &FastCopy,
) {
    let in_region: HashSet<BlockId> = region.iter().copied().collect();
    for cell in cells_of(func, region, cells) {
        // a cell only the loop stores holds nothing on the way in
        let stored_outside = (0..outside)
            .filter(|bi| !in_region.contains(&BlockId(*bi as u32)))
            .any(|bi| func.blocks[bi].insts.iter().any(|i| i.result == Some(cell)));
        if !stored_outside {
            continue;
        }
        let from = entry_value_in(func, &in_region, header, outside, cell, cells)
            .unwrap_or(Operand::Value(cell));
        let ty = func.values[cell.0 as usize].ty;
        func.blocks[pass.0 as usize].insts.push(Inst {
            result: Some(copy.values[&cell]),
            kind: InstKind::Copy(ty, from),
            origin: None,
        });
    }
}

/// What every block entering the loop last stored into `cell`, when
/// they all stored the same thing.
pub(crate) fn entry_value(
    func: &Function,
    region: &HashSet<BlockId>,
    header: BlockId,
    cell: ValueId,
    cells: &HashSet<ValueId>,
) -> Option<Operand> {
    entry_value_in(func, region, header, func.blocks.len(), cell, cells)
}

fn entry_value_in(
    func: &Function,
    region: &HashSet<BlockId>,
    header: BlockId,
    outside: usize,
    cell: ValueId,
    cells: &HashSet<ValueId>,
) -> Option<Operand> {
    let mut given = (0..outside)
        .filter(|bi| !region.contains(&BlockId(*bi as u32)))
        .filter(|bi| successors(&func.blocks[*bi].term).any(|s| s == header))
        .map(|bi| stored(func, bi, cell, cells));
    let first = given.next().flatten()?;
    given.all(|g| g == Some(first)).then_some(first)
}

/// Mint the out cells, point the readers at them, and store into
/// them on every edge that leaves either copy for a reader.
pub(super) fn write_out(
    func: &mut Function,
    region: &[BlockId],
    out: &[ValueId],
    readers: &[BlockId],
    copy: &FastCopy,
) {
    let cell_of: Vec<(ValueId, ValueId)> = out
        .iter()
        .map(|c| {
            let ty = func.values[c.0 as usize].ty;
            (*c, push_value(&mut func.values, ty))
        })
        .collect();
    let to_out: HashMap<ValueId, Operand> = cell_of
        .iter()
        .map(|(c, o)| (*c, Operand::Value(*o)))
        .collect();
    for b in readers {
        let block = &mut func.blocks[b.0 as usize];
        for inst in block.insts.iter_mut() {
            rewrite_operands(&mut inst.kind, &to_out);
        }
        block.term = remap_term_operand(block.term.clone(), &to_out);
    }
    let mut leaving: Vec<(BlockId, bool)> = region
        .iter()
        .flat_map(|b| [(*b, false), (copy.blocks[b], true)])
        .collect();
    leaving.sort_by_key(|(b, _)| b.0);
    let inside: HashSet<BlockId> = leaving.iter().map(|(b, _)| *b).collect();
    let reach = crate::cfg_edit::reaching(func, readers, &inside);
    for (b, fast) in leaving {
        let targets: Vec<BlockId> = successors(&func.blocks[b.0 as usize].term)
            .filter(|t| !inside.contains(t) && reach.contains(t))
            .collect();
        for target in targets {
            let exit = BlockId(func.blocks.len() as u32);
            let insts = cell_of
                .iter()
                .map(|(cell, o)| Inst {
                    result: Some(*o),
                    kind: InstKind::Copy(
                        func.values[cell.0 as usize].ty,
                        Operand::Value(if fast { copy.values[cell] } else { *cell }),
                    ),
                    origin: None,
                })
                .collect();
            func.blocks.push(Block {
                id: exit,
                insts,
                term: Terminator::Br(target),
            });
            retarget_edge(&mut func.blocks[b.0 as usize].term, target, exit);
        }
    }
}
