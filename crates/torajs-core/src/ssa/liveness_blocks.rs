//! Block-level live-in / live-out sets — the least solution of the
//! backward data-flow equations over the block CFG:
//!
//!   live_in[B]  = use[B] ∪ (live_out[B] \ def[B])
//!   live_out[B] = ∪{live_in[s] : s ∈ succ(B)}
//!
//! Solved per value by path exploration (Brandner et al., "Computing
//! Liveness Sets for SSA-Form Programs"): from every block with an
//! upward-exposed use, walk predecessors until a block that defines the
//! value. φ cells carry several defs, and any one of them stops the
//! walk, exactly as `def[B]` kills in the equations. The work is linear
//! in the total size of the sets instead of blocks × values per round.
//!
//! Positions use the global inst-slot numbering of codegen's
//! `liveness::compute_intervals`: one index per inst, then one for the
//! terminator. Interval extension and copy coalescing both read the
//! result.

use super::id_hash::HashMap;
use super::{Function, Operand, Terminator, ValueId, visit_value_operands};

/// Per-block liveness, index-aligned with `func.blocks`.
pub struct BlockLiveness {
    /// Global slot of the block's first inst (== its terminator slot
    /// when the block has no insts).
    pub start: Vec<u32>,
    /// Global slot of the block's terminator.
    pub end: Vec<u32>,
    pub live_in: Vec<ValueSet>,
    pub live_out: Vec<ValueSet>,
}

/// A set of value ids, stored as its members in ascending order.
#[derive(Clone, Default)]
pub struct ValueSet {
    members: Vec<u32>,
}

impl ValueSet {
    /// The members, ascending.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.members.iter().copied()
    }
}

/// Compressed adjacency: the items of key `k` are
/// `items[offsets[k]..offsets[k + 1]]`.
struct Csr {
    offsets: Vec<u32>,
    items: Vec<u32>,
}

impl Csr {
    /// Bucket `(key, item)` pairs by key, keeping their order per key.
    fn group(keys: usize, pairs: &[(u32, u32)]) -> Self {
        let mut offsets = vec![0u32; keys + 1];
        for &(k, _) in pairs {
            offsets[k as usize + 1] += 1;
        }
        for k in 0..keys {
            offsets[k + 1] += offsets[k];
        }
        let mut fill = offsets.clone();
        let mut items = vec![0u32; pairs.len()];
        for &(k, item) in pairs {
            items[fill[k as usize] as usize] = item;
            fill[k as usize] += 1;
        }
        Csr { offsets, items }
    }

    fn get(&self, k: usize) -> &[u32] {
        &self.items[self.offsets[k] as usize..self.offsets[k + 1] as usize]
    }
}

/// One linear pass over the blocks: slot numbering, CFG edges, and the
/// `(value, block)` pairs of upward-exposed uses and of defs.
struct Scan {
    start: Vec<u32>,
    end: Vec<u32>,
    /// `(succ, block)`, so grouping by the first field gives preds.
    edges: Vec<(u32, u32)>,
    uses: Vec<(u32, u32)>,
    defs: Vec<(u32, u32)>,
    /// Per value, the last block that defined / used it.
    def_in: Vec<u32>,
    use_in: Vec<u32>,
}

impl Scan {
    fn read(&mut self, v: u32, b: u32) {
        let v_ = v as usize;
        if self.def_in[v_] != b && self.use_in[v_] != b {
            self.use_in[v_] = b;
            self.uses.push((v, b));
        }
    }

    fn write(&mut self, v: u32, b: u32) {
        if self.def_in[v as usize] != b {
            self.def_in[v as usize] = b;
            self.defs.push((v, b));
        }
    }
}

fn scan(func: &Function) -> Scan {
    let n = func.blocks.len();
    // BlockId may be non-contiguous, so map it to a position.
    let pos: HashMap<u32, u32> = (func.blocks.iter().enumerate())
        .map(|(i, b)| (b.id.0, i as u32))
        .collect();
    let mut s = Scan {
        start: vec![0; n],
        end: vec![0; n],
        edges: Vec::new(),
        uses: Vec::new(),
        defs: Vec::new(),
        def_in: vec![u32::MAX; func.values.len()],
        use_in: vec![u32::MAX; func.values.len()],
    };
    let mut cursor: u32 = 0;
    for (i, block) in func.blocks.iter().enumerate() {
        let b = i as u32;
        s.start[i] = cursor;
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| s.read(v.0, b));
            if let Some(r) = inst.result {
                s.write(r.0, b);
            }
            cursor += 1;
        }
        visit_terminator_values(&block.term, |v| s.read(v.0, b));
        s.end[i] = cursor;
        cursor += 1;

        let mut edge = |t: u32| s.edges.extend(pos.get(&t).map(|&succ| (succ, b)));
        match &block.term {
            Terminator::Br(t) => edge(t.0),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                edge(then_blk.0);
                edge(else_blk.0);
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
    s
}

/// Call `f` with the value a terminator reads, if any.
fn visit_terminator_values(term: &Terminator, mut f: impl FnMut(ValueId)) {
    match term {
        Terminator::Ret(Some(Operand::Value(v)))
        | Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        } => f(*v),
        _ => {}
    }
}

pub fn compute_block_liveness(func: &Function) -> BlockLiveness {
    let n = func.blocks.len();
    let values = func.values.len();
    let s = scan(func);
    let preds = Csr::group(n, &s.edges);
    let uses = Csr::group(values, &s.uses);
    let defs = Csr::group(values, &s.defs);

    let mut live_in = vec![ValueSet::default(); n];
    let mut live_out = vec![ValueSet::default(); n];
    // per block, the last value marked live-in / live-out / defined
    let mut in_mark = vec![u32::MAX; n];
    let mut out_mark = vec![u32::MAX; n];
    let mut def_mark = vec![u32::MAX; n];
    let mut work: Vec<u32> = Vec::new();
    // ascending values keep every set's members sorted
    for v in 0..values as u32 {
        for &b in defs.get(v as usize) {
            def_mark[b as usize] = v;
        }
        work.extend_from_slice(uses.get(v as usize));
        while let Some(b) = work.pop() {
            let b = b as usize;
            if in_mark[b] == v {
                continue;
            }
            in_mark[b] = v;
            live_in[b].members.push(v);
            for &p in preds.get(b) {
                let p_ = p as usize;
                if out_mark[p_] != v {
                    out_mark[p_] = v;
                    live_out[p_].members.push(v);
                }
                if def_mark[p_] != v && in_mark[p_] != v {
                    work.push(p);
                }
            }
        }
    }

    BlockLiveness {
        start: s.start,
        end: s.end,
        live_in,
        live_out,
    }
}

#[cfg(test)]
mod tests;
