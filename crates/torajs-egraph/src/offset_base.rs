//! Constant offsets out of a loop's byte and halfword reads —
//! `load_u8(p, i + c)` with `p` invariant in the loop becomes
//! `load_u8(p + c, i)`, `load_u16(p, i + c)` becomes
//! `load_u16(p + 2c, i)`, and `p + c` is computed once ahead of the
//! loop (the reassociation LLVM's SeparateConstOffsetFromGEP does so
//! that LICM can take the invariant part out).
//!
//! The reads are the inline `charCodeAt` and the split / trim scans:
//! the index is the payload offset past the 16-byte string header, so
//! every read inside a scan loop paid one add on the address before
//! the register-offset load. Both encodings' reads of one receiver
//! share one advanced base (16 bytes is 8 halfwords).
//!
//! Conditions: the `i + c` sum is used only as the index of such reads
//! (otherwise it stays, and moving the reads saves nothing); the base
//! is a value defined outside the loop (a parameter, or a single
//! definition in a block that dominates the loop). The advanced base
//! goes at the end of the preheader of the outermost loop that leaves
//! `p` invariant — a new block on the loop's entry edges when no block
//! outside enters only the loop, so the base is computed once per
//! entry and is live across nothing but the loop. It is a raw address
//! into memory `p` owns, read at exactly the points the original reads
//! were, so the lifetime of `p` does not change.
//!
//! Late in the pipeline, after every pass that matches a string's
//! uses (append forwarding, concat hoisting): they read a load through
//! the string itself and would take the address arithmetic for an
//! escape.

use crate::id_hash::HashMap;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use crate::float_demote::cfg::{retarget_edge, successors, term_operand_value};
use crate::float_demote::push_value;

#[derive(Debug, Default)]
pub struct OffsetBaseStats {
    pub folded_reads: usize,
    pub bases: usize,
    /// Loops given a preheader to hold their bases.
    pub preheaders: usize,
}

pub fn fold_offset_bases(module: &mut Module) -> OffsetBaseStats {
    let mut stats = OffsetBaseStats::default();
    // most of a module is runtime declarations: skipped before any call
    for func in module.funcs.iter_mut().filter(|f| !f.is_declaration()) {
        fold_function(func, &mut stats);
    }
    stats
}

/// A read at `(block, inst)` of `base` whose index is `sum = i + c`.
struct Read {
    at: (usize, usize),
    base: ValueId,
    sum: ValueId,
    /// Filled in once the sum's definition is found.
    sum_at: (usize, usize),
    i: Operand,
    /// Bytes the constant moves the address by.
    bytes: i64,
    /// The block (position) of the base's one definition, `None` for a
    /// parameter.
    base_def: Option<usize>,
}

/// The definitions and uses of a few values, from one walk over the
/// function. The pass runs on every function with a code-unit read, so
/// it books only the values its reads name.
struct Defs {
    /// Per value id: its index among the booked values, or `u32::MAX`.
    slot: Vec<u32>,
    /// Per booked value: how many definitions, the first one's
    /// position, the sum `i + c` it computes when it is one, and how
    /// many operands name it.
    n: Vec<u32>,
    at: Vec<(usize, usize)>,
    sum: Vec<Option<(Operand, i64)>>,
    uses: Vec<u32>,
}

impl Defs {
    fn of(func: &Function, ids: impl IntoIterator<Item = ValueId>, count_uses: bool) -> Self {
        let mut slot = vec![u32::MAX; func.values.len()];
        let mut k = 0;
        for v in ids {
            let s = &mut slot[v.0 as usize];
            if *s == u32::MAX {
                *s = k;
                k += 1;
            }
        }
        let k = k as usize;
        let mut out = Defs {
            slot,
            n: vec![0; k],
            at: vec![(0, 0); k],
            sum: vec![None; k],
            uses: vec![0; k],
        };
        for (bi, block) in func.blocks.iter().enumerate() {
            for (ii, inst) in block.insts.iter().enumerate() {
                if count_uses {
                    visit_value_operands(&inst.kind, |v| out.count_use(v));
                }
                let Some(r) = inst.result else { continue };
                let x = out.slot[r.0 as usize];
                if x == u32::MAX {
                    continue;
                }
                let x = x as usize;
                if out.n[x] == 0 {
                    out.at[x] = (bi, ii);
                    out.sum[x] = (func.values[r.0 as usize].ty == Type::I64)
                        .then(|| offset_sum(&inst.kind))
                        .flatten();
                }
                out.n[x] += 1;
            }
            if count_uses && let Some(v) = term_operand_value(&block.term) {
                out.count_use(v);
            }
        }
        out
    }

    fn count_use(&mut self, v: ValueId) {
        let x = self.slot[v.0 as usize];
        if x != u32::MAX {
            self.uses[x as usize] += 1;
        }
    }

    fn ix(&self, v: ValueId) -> usize {
        self.slot[v.0 as usize] as usize
    }

    /// The block (position) of `v`'s one definition: `Some(None)` for a
    /// parameter nothing redefines, `None` for a value defined more than
    /// once.
    fn single(&self, func: &Function, v: ValueId) -> Option<Option<usize>> {
        let x = self.ix(v);
        match (func.params.contains(&v), self.n[x]) {
            (true, 0) => Some(None),
            (false, 1) => Some(Some(self.at[x].0)),
            _ => None,
        }
    }

    /// Keeps the reads on a sum only when they are all its uses: a sum
    /// something else reads stays, and moving its reads saves nothing.
    fn keep_sole_uses(&self, reads: &mut Vec<Read>) {
        let mut on_sum = vec![0u32; self.n.len()];
        for r in reads.iter() {
            on_sum[self.ix(r.sum)] += 1;
        }
        reads.retain(|r| self.uses[self.ix(r.sum)] == on_sum[self.ix(r.sum)]);
    }
}

/// `i + c` as `(i, c)`; the caller checks the sum is an `i64`.
fn offset_sum(kind: &InstKind) -> Option<(Operand, i64)> {
    match kind {
        InstKind::BinOp(BinOp::Add, i @ Operand::Value(_), Operand::ConstI64(c))
        | InstKind::BinOp(BinOp::Add, Operand::ConstI64(c), i @ Operand::Value(_)) => {
            Some((*i, *c))
        }
        _ => None,
    }
}

fn fold_function(func: &mut Function, stats: &mut OffsetBaseStats) {
    let reads = candidates(func);
    if reads.is_empty() {
        return;
    }
    let plans = place(func, &reads);
    if !plans.moves.is_empty() {
        apply(func, &reads, plans, stats);
    }
}

/// The reads whose index is a sum `i + c` of a single definition, over a
/// base of a single definition (or a parameter).
fn candidates(func: &Function) -> Vec<Read> {
    let mut reads = Vec::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            let (base, sum, scale) = match inst.kind {
                InstKind::LoadU8Dyn(Operand::Value(b), Operand::Value(j)) => (b, j, 1),
                InstKind::LoadU16Dyn(Operand::Value(b), Operand::Value(j)) => (b, j, 2),
                _ => continue,
            };
            // a string's bytes or a raw buffer: an object, inline value
            // or regex as an add operand is taken for an escape
            let addressable = matches!(func.values[base.0 as usize].ty, Type::Str | Type::Ptr);
            if base != sum && addressable {
                reads.push(Read {
                    at: (bi, ii),
                    base,
                    sum,
                    sum_at: (0, 0),
                    i: Operand::ConstI64(0),
                    bytes: scale,
                    base_def: None,
                });
            }
        }
    }
    if reads.is_empty() {
        return reads;
    }
    let defs = Defs::of(func, reads.iter().flat_map(|r| [r.base, r.sum]), true);
    // over every read first, which rules out most sums before anything
    // else is looked at; again at the end over the reads left
    defs.keep_sole_uses(&mut reads);
    reads.retain_mut(|r| {
        let x = defs.ix(r.sum);
        let (Some((i, c)), 1) = (defs.sum[x], defs.n[x]) else {
            return false;
        };
        let Some(base_def) = defs.single(func, r.base) else {
            return false;
        };
        r.sum_at = defs.at[x];
        r.i = i;
        r.bytes = r.bytes.wrapping_mul(c);
        r.base_def = base_def;
        i != Operand::Value(r.base)
    });
    // a counter cell copies write must hold at the read what it held at
    // the sum: checked within the block, or by its having one definition
    let far: Vec<ValueId> = (reads.iter())
        .filter(|r| !same_block_value(func, r))
        .filter_map(|r| match r.i {
            Operand::Value(v) => Some(v),
            _ => None,
        })
        .collect();
    if !far.is_empty() {
        let defs = Defs::of(func, far, false);
        reads.retain(|r| match r.i {
            Operand::Value(v) if !same_block_value(func, r) => defs.single(func, v).is_some(),
            _ => true,
        });
    }
    // the loop analysis below costs more than everything above: only a
    // read some cycle runs without passing the base's definition has a
    // loop to take the base out of
    if !reads.is_empty() {
        let pos = positions(func);
        let mut seen: HashMap<(usize, Option<usize>), bool> = HashMap::default();
        reads.retain(|r| {
            let key = (r.at.0, r.base_def);
            *seen
                .entry(key)
                .or_insert_with(|| cycles_past(func, &pos, key.0, key.1))
        });
    }
    defs.keep_sole_uses(&mut reads);
    reads
}

/// Per block id, the block's position in `func.blocks`.
fn positions(func: &Function) -> Vec<usize> {
    let span = (func.blocks.iter()).map(|b| b.id.0 as usize + 1).max();
    let mut pos = vec![usize::MAX; span.unwrap_or(0)];
    for (bi, b) in func.blocks.iter().enumerate() {
        pos[b.id.0 as usize] = bi;
    }
    pos
}

/// Is there a cycle through block `at` that does not pass block
/// `avoid`? With `avoid` dominating `at` (a definition dominates its
/// uses), such a cycle lies in a loop around `at` that leaves `avoid`
/// outside: the loop's header is on the cycle, and a path into the
/// header from outside the loop reaches `at` without `avoid`, so
/// `avoid` inside the loop would not dominate `at`.
fn cycles_past(func: &Function, pos: &[usize], at: usize, avoid: Option<usize>) -> bool {
    if avoid == Some(at) {
        return false;
    }
    let succ = |b: usize| -> [Option<usize>; 2] {
        match func.blocks[b].term {
            Terminator::Br(t) => [Some(pos[t.0 as usize]), None],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => [
                Some(pos[then_blk.0 as usize]),
                Some(pos[else_blk.0 as usize]),
            ],
            Terminator::Ret(_) | Terminator::Unreachable => [None, None],
        }
    };
    let mut seen = vec![false; func.blocks.len()];
    let mut stack: Vec<usize> = succ(at).into_iter().flatten().collect();
    while let Some(b) = stack.pop() {
        if b == at {
            return true;
        }
        if Some(b) == avoid || seen[b] {
            continue;
        }
        seen[b] = true;
        stack.extend(succ(b).into_iter().flatten());
    }
    false
}

/// Where a moved read's advanced base is computed: the end of the
/// loop's preheader — the one block outside the loop that enters it,
/// when it enters nothing else — or a new block on the loop's entry
/// edges (`entries[k]`), made when the first base needs it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Pre {
    Block(usize),
    New(usize),
}

/// Per read that moves, its index in `reads` and where its base goes;
/// and per loop that needs a new preheader, its header and the
/// positions of the blocks outside it that branch to it.
struct Plans {
    moves: Vec<(usize, Pre)>,
    entries: Vec<(BlockId, Vec<usize>)>,
}

/// The base goes ahead of the outermost loop around the read that
/// leaves the base invariant. The base's definition, outside that loop
/// and dominating the read, dominates the loop's header and so every
/// block entering it. A sum with a read that cannot move keeps every
/// read on it as it is.
fn place(func: &Function, reads: &[Read]) -> Plans {
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    let id = |bi: usize| func.blocks[bi].id;
    let mut entries: Vec<(BlockId, Vec<usize>)> = Vec::new();
    let mut target = |r: &Read| -> Option<Pre> {
        let at = id(r.at.0);
        let def = r.base_def.map(id);
        let outer = (loops.loops().iter())
            .filter(|l| l.contains(&at) && def.is_none_or(|d| !l.contains(&d)))
            .max_by_key(|l| l.body.len())?;
        let h = outer.header;
        if let Some(k) = entries.iter().position(|(e, _)| *e == h) {
            return Some(Pre::New(k));
        }
        let from: Vec<usize> = (0..func.blocks.len())
            .filter(|&b| {
                !outer.contains(&id(b)) && successors(&func.blocks[b].term).any(|s| s == h)
            })
            .collect();
        match from.as_slice() {
            [] => None,
            [b] if func.blocks[*b].term == Terminator::Br(h) => Some(Pre::Block(*b)),
            _ => {
                entries.push((h, from));
                Some(Pre::New(entries.len() - 1))
            }
        }
    };
    let plans: Vec<Option<Pre>> = reads.iter().map(&mut target).collect();
    let moves = (plans.iter().enumerate())
        .filter_map(|(n, p)| {
            let pre = (*p)?;
            let stays = |m: usize| reads[m].sum == reads[n].sum && plans[m].is_none();
            (!(0..reads.len()).any(stays)).then_some((n, pre))
        })
        .collect();
    Plans { moves, entries }
}

/// A block on every edge from `from` (positions) into `header`; its
/// position.
fn new_preheader(func: &mut Function, header: BlockId, from: &[usize]) -> usize {
    let id = BlockId(func.blocks.iter().map(|b| b.id.0 + 1).max().unwrap_or(0));
    for &b in from {
        retarget_edge(&mut func.blocks[b].term, header, id);
    }
    func.blocks.push(Block {
        id,
        insts: Vec::new(),
        term: Terminator::Br(header),
    });
    func.blocks.len() - 1
}

/// One advanced base per (preheader, base, offset) at the end of its
/// preheader, each read moved onto it, and the sums they no longer use
/// removed.
fn apply(func: &mut Function, reads: &[Read], plans: Plans, stats: &mut OffsetBaseStats) {
    let mut made: Vec<Option<usize>> = vec![None; plans.entries.len()];
    let mut advanced: HashMap<(usize, ValueId, i64), ValueId> = HashMap::default();
    let mut tails: Vec<(usize, Inst)> = Vec::new();
    let mut dead: Vec<(usize, usize)> = Vec::new();
    for (n, pre) in plans.moves {
        let r = &reads[n];
        let pre = match pre {
            Pre::Block(b) => b,
            Pre::New(k) => *made[k].get_or_insert_with(|| {
                let (h, from) = &plans.entries[k];
                stats.preheaders += 1;
                new_preheader(func, *h, from)
            }),
        };
        let base = match advanced.get(&(pre, r.base, r.bytes)) {
            Some(v) => *v,
            None => {
                let v = push_value(&mut func.values, Type::Ptr);
                let kind = InstKind::BinOp(
                    BinOp::Add,
                    Operand::Value(r.base),
                    Operand::ConstI64(r.bytes),
                );
                let inst = Inst {
                    result: Some(v),
                    kind,
                    origin: None,
                };
                tails.push((pre, inst));
                stats.bases += 1;
                advanced.insert((pre, r.base, r.bytes), v);
                v
            }
        };
        let (at, i) = (r.sum_at, r.i);
        match &mut func.blocks[r.at.0].insts[r.at.1].kind {
            InstKind::LoadU8Dyn(b, idx) | InstKind::LoadU16Dyn(b, idx) => {
                *b = Operand::Value(base);
                *idx = i;
            }
            _ => unreachable!("collected as a dynamic code-unit read"),
        }
        if !dead.contains(&at) {
            dead.push(at);
        }
        stats.folded_reads += 1;
    }
    dead.sort_unstable_by(|a, b| b.cmp(a));
    for (bi, ii) in dead {
        func.blocks[bi].insts.remove(ii);
    }
    for (pre, inst) in tails {
        func.blocks[pre].insts.push(inst);
    }
}

/// Does the counter `i` read at the read hold what it held at the sum,
/// judged within the block: the read follows the sum with no write of
/// `i` between.
fn same_block_value(func: &Function, r: &Read) -> bool {
    let Operand::Value(v) = r.i else {
        return true;
    };
    let (sum, read) = (r.sum_at, r.at);
    sum.0 == read.0
        && sum.1 < read.1
        && func.blocks[sum.0].insts[sum.1 + 1..read.1]
            .iter()
            .all(|inst| inst.result != Some(v))
}

#[cfg(test)]
mod tests;
