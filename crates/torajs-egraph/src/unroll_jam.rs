//! Unroll-and-jam of a counted loop around an inner `while` loop
//! (Allen & Kennedy, *Optimizing Compilers for Modern Architectures*
//! §8.4; LLVM `LoopUnrollAndJam`), for inner loops whose trip counts
//! differ from one outer iteration to the next.
//!
//! ```text
//! for (i = s; i <= n; i += C) { pre(i); while (c) body; post }
//! ```
//!
//! While `L` outer iterations remain (`L` lanes, chosen per loop), a
//! group runs `pre` for `i, i+C, …`, steps all `L` inner loops together
//! while every one of them is still running, then lets each lane finish
//! its own inner loop and run its own `post`, lane by lane in iteration
//! order. The fused loop keeps `L` independent recurrences in flight, so
//! an inner loop bound by one long-latency chain (a divide per step:
//! Euclid's gcd, digit and radix loops; a multiply-add: Collatz steps)
//! runs at the units' throughput instead of its latency. Fewer than `L`
//! remaining iterations run through the untouched original loop.
//!
//! Why the result cannot differ:
//! - every lane executes exactly its own iteration's instruction
//!   sequence; the only change is that later lanes run part of `pre`
//!   and the inner loop before earlier lanes reach `post`. That work is
//!   pure, trap-free, reads no memory and no value `post` writes, so it
//!   is unobservable; if an earlier lane's `post` leaves the loop, it
//!   is dropped.
//! - the fused loop stops when any lane's inner loop is done, so it
//!   never runs longer than lane 0's own inner loop: an inner loop
//!   that does not terminate hangs at the same iteration as before.
//!   An inner loop with a guard that leaves only it (a `break`, an
//!   inlined early `return`) is stepped under masks instead (`masked`):
//!   the group runs while the earliest unfinished lane still steps.
//! - `post` (reductions and their overflow guards, calls, stores) runs
//!   once per lane in iteration order: sums keep their order and each
//!   exit sees the state the original saw at that point.
//! - a group starts only when all its iterations pass the loop test
//!   (`i <= n - (L-1)·C`, the subtraction checked for wrap once
//!   before the loop, `n` defined outside it). A guard in `pre` that
//!   would leave the loop (an overflow bail) sends the whole group back
//!   to the original loop when any lane's copy fires: nothing has been
//!   written by then.
//!
//! The IR has no φ nodes (loop cells are multi-def `Copy`), so lane
//! state is per-lane renamed cells in the fused part and the original
//! cells again, copied in, when a lane finishes on its own.
//!
//! Before loop_rotate. `TORAJS_UNROLL_JAM_OFF=1` skips,
//! `TORAJS_UNROLL_JAM_STATS=1` dumps counters.

mod chain;
mod clone;
mod emit;
mod fit;
mod flow;
mod guards;
mod heads;
mod masked;
mod plan;
mod retire;
#[cfg(test)]
mod tests;

use crate::id_hash::HashSet;

use torajs_core::ssa::{BlockId, IPred, Module, Operand, ValueId};

/// Outer iterations in flight per group: at least this many, and more
/// where the inner recurrence is long enough to keep more divides in
/// flight (see `fit::lane_count`).
pub const MIN_LANES: usize = 4;
/// At most this many: each lane keeps its cells in registers.
pub const MAX_LANES: usize = 8;
/// General registers every lane's own state may take together, out of
/// the 21 the allocator hands out; the rest hold shared values, each
/// step's temporaries and the outer loop. Bounds the lane count of a
/// lockstep group (`fit::lane_count`) and of a masked one
/// (`masked::masked_lanes`).
const LANE_GPR_BUDGET: usize = 18;
const MAX_PRE_INSTS: usize = 16;
const MAX_INNER_INSTS: usize = 24;
const MAX_POST_INSTS: usize = 64;
const MAX_CHAIN_BLOCKS: usize = 8;
const MAX_STEP: i64 = 1 << 20;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UnrollJamStats {
    /// Outer loops given a jammed group path.
    pub loops_jammed: u32,
    /// Of those, the ones whose group steps its lanes under masks.
    pub loops_masked: u32,
}

pub fn jam_module(module: &mut Module) -> UnrollJamStats {
    jam_functions(module).1
}

/// Loops jammed in each function of the module, by function index,
/// and the module's counters.
pub fn jam_functions(module: &mut Module) -> (Vec<u32>, UnrollJamStats) {
    let mut stats = UnrollJamStats::default();
    let mut jammed = vec![0; module.funcs.len()];
    for (func, count) in module.funcs.iter_mut().zip(&mut jammed) {
        if func.is_declaration() {
            continue;
        }
        // an arm select_form emptied still branches to its join, and as
        // a predecessor would break the straight chains planned below
        crate::branch_fold::sweep_unreachable_blocks(func);
        // the original loop survives as the remainder path, so a
        // header once handled must not be picked again
        let mut done: HashSet<BlockId> = HashSet::default();
        while let Some(plan) = plan::find_plan(func, &done) {
            done.insert(plan.header);
            emit::apply(func, &plan);
            *count += 1;
            stats.loops_jammed += 1;
            stats.loops_masked += u32::from(plan.masked);
        }
    }
    (jammed, stats)
}

/// One outer loop the rewrite takes, with its regions named.
pub(crate) struct Plan {
    pub header: BlockId,
    /// Blocks between the header and the inner header, in order.
    pub pre_blocks: Vec<BlockId>,
    /// The header's exit compare, when nothing but its branch reads it.
    pub skip: Option<ValueId>,
    pub iv: ValueId,
    pub step: i64,
    /// Outer iterations in flight per group.
    pub lanes: usize,
    /// `iv <pred> bound` keeps looping; `Slt` or `Sle`.
    pub pred: IPred,
    pub bound: Operand,
    pub inner_header: BlockId,
    /// The inner header's cond keeps the inner loop going when true.
    pub inner_on_true: bool,
    /// Inner body blocks after the inner header, in order.
    pub chain: Vec<BlockId>,
    /// The inner body has a guard leaving the inner loop for `post`:
    /// the group steps its lanes under masks (see `masked`).
    pub masked: bool,
    /// Under masks, the cells a lane keeps from the step it leaves in,
    /// when fewer than the inner loop writes (see `retire`).
    pub exit_cells: Option<Vec<ValueId>>,
    /// Everything else in the outer body, the latch included.
    pub post: Vec<BlockId>,
    pub body: HashSet<BlockId>,
    /// Copy cells defined before the inner loop: one lane's state.
    pub lane_cells: Vec<ValueId>,
    /// Non-cell loop values read after the loop; lane clones redefine
    /// them under the same id.
    pub escaping: HashSet<ValueId>,
    /// Values of `pre` and the inner loop every lane computes alike.
    pub shared: HashSet<ValueId>,
}

impl Plan {
    /// Header then pre blocks: the part every lane runs before its
    /// inner loop.
    pub(crate) fn pre_order(&self) -> Vec<BlockId> {
        std::iter::once(self.header)
            .chain(self.pre_blocks.iter().copied())
            .collect()
    }
    pub(crate) fn inner_order(&self) -> Vec<BlockId> {
        std::iter::once(self.inner_header)
            .chain(self.chain.iter().copied())
            .collect()
    }
}
