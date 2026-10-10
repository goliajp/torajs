//! Unroll a divide-bound inner loop by two, every exit kept, and take
//! the two copies' float quotients from one two-lane divide.
//!
//! ```text
//!                                         P:  xv = splat x; yv = pair (sitofp i), (sitofp i) + C
//! H:  c = <test>; cond_br c, B, X         H:  c = <test>; cond_br c, B, X
//! B:  yf = sitofp i                       B:  qv = vec.fdiv xv, yv
//!     q = fdiv x, yf                          tv = vec.fptosi qv; t = lane 0 tv
//!     t = fptosi q                            ... cond_br z, XF, L
//!     ... cond_br z, XF, L                L:  i = i + C; br H'
//! L:  i = i + C; br H                     H': c' = <test>; cond_br c', B', X
//!                                         B': t' = lane 1 tv; ... cond_br z', XF, L'
//!                                         L': i = i + C; yv = yv + splat 2C; br H
//! ```
//!
//! The loop's trip count is not computable (a trial-division test is
//! `i * i <= n`), so the unroll keeps the header test between the
//! copies instead of peeling a remainder (LLVM `LoopUnroll` with
//! `Force` and no runtime remainder; the textbook "unroll with all
//! exits"), then pairs the copies' independent `sitofp → fdiv →
//! fptosi` chains lane-wise (SLP vectorization, Larsen & Amarasinghe
//! 2000). The second lane's quotient is computed before the first
//! copy's exit test: it is pure and trap-free, and read only when the
//! second copy runs, which happens exactly when the original loop ran
//! that iteration. The M-series f64 divider takes a two-lane divide
//! at the one-lane rate, so two trial divisions cost one.
//!
//! The divisor lanes are a float counter beside the integer one when
//! the interval facts keep the counter and two steps past it inside
//! ±2^53: every lane value is then an exact integer and `yv + 2C` is
//! the conversion of the stepped counter. Without that bound the
//! first copy converts `i` and `i + C` each trip and pairs them.
//!
//! Which loops qualify is decided in `plan`: an innermost top-tested
//! loop whose body is one straight chain of pure instructions with at
//! most one exit per block, one `fdiv` of a loop-invariant value and
//! the conversion of a counter stepped by a constant, read once by an
//! `fptosi`; no value but a loop cell is read after the loop. The IR
//! has no φ nodes (loop cells are multi-def `Copy`), so the second
//! copy reads the cells the first copy's latch wrote.
//!
//! A trial-division loop whose one body exit is the divisibility test
//! and whose counter is its one cell is instead searched several
//! divisors a step ahead of the loop, which is kept as it was
//! (`search`).
//!
//! After unroll_jam, in the functions it left alone (a jammed loop
//! runs four quotients a step already) and before loop_rotate (the second
//! copy's header test rotates into its latch like the first's).
//! `TORAJS_UNROLL_PAIR_OFF=1` skips, `TORAJS_UNROLL_PAIR_STATS=1`
//! dumps counters.

mod emit;
mod plan;
mod search;
#[cfg(test)]
mod tests;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Module, Operand, ValueId};

use crate::interval::NumFact;

const MAX_CHAIN_BLOCKS: usize = 8;
const MAX_INSTS: usize = 32;
const MAX_STEP: i64 = 1 << 20;
const F64_EXACT: i128 = 1 << 53;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UnrollPairStats {
    /// Loops unrolled and paired.
    pub loops_paired: u32,
    /// Of those, the ones whose divisor lanes are a float counter.
    pub float_counters: u32,
    /// Trial-division loops given a lane search ahead of them.
    pub loops_searched: u32,
}

pub fn pair_module(
    module: &mut Module,
    facts: Option<&[HashMap<ValueId, NumFact>]>,
    jammed: &[u32],
) -> UnrollPairStats {
    debug_assert!(facts.is_none_or(|f| f.len() == module.funcs.len()));
    let mut stats = UnrollPairStats::default();
    for (fi, func) in module.funcs.iter_mut().enumerate() {
        if func.is_declaration() || jammed.get(fi).is_some_and(|n| *n > 0) {
            continue;
        }
        crate::branch_fold::sweep_unreachable_blocks(func);
        // a paired loop's second copy is a new header the analysis
        // would list next round; a header once handled is not retaken
        let mut done: HashSet<BlockId> = HashSet::default();
        let known = facts.and_then(|f| f.get(fi));
        // the searched loop stays as it was, to replay a group
        while let Some(sp) = search::find_plan(func, known, &done) {
            done.insert(sp.header);
            done.insert(search::apply(func, &sp));
            stats.loops_searched += 1;
        }
        while let Some(mut plan) = plan::find_plan(func, &done) {
            done.insert(plan.header);
            let reach = F64_EXACT - 2 * plan.step as i128;
            plan.float_counter = known
                .and_then(|fm| fm.get(&plan.iv))
                .is_some_and(|f| f.lo >= -reach && f.hi <= reach);
            stats.float_counters += u32::from(plan.float_counter);
            emit::apply(func, &plan);
            stats.loops_paired += 1;
        }
    }
    stats
}

/// One loop the rewrite takes.
pub(crate) struct Plan {
    pub header: BlockId,
    /// The one block outside the loop the header is entered from.
    pub preheader: BlockId,
    /// The divisor lanes step as a float counter (decided by the
    /// counter's interval fact).
    pub float_counter: bool,
    /// Body blocks after the header, in order; the last is the latch.
    pub chain: Vec<BlockId>,
    /// The counter cell and its step.
    pub iv: ValueId,
    pub step: i64,
    /// The `iv + step` the latch copies, and where it sits.
    pub step_add: ValueId,
    pub step_at: (BlockId, usize),
    /// The block holding the quotient chain and the chain's positions.
    pub site: BlockId,
    pub fdiv_at: usize,
    pub fptosi_at: usize,
    /// `sitofp iv`, the quotient and its integer.
    pub yf: ValueId,
    pub q: ValueId,
    pub t: ValueId,
    /// The loop-invariant operand, and whether it is the dividend.
    pub inv: Operand,
    pub inv_is_lhs: bool,
    /// Values every def of which is a `Copy`: kept under their own id
    /// by the second copy.
    pub cells: HashSet<ValueId>,
}

impl Plan {
    /// Header then chain: the whole loop in order.
    pub(crate) fn order(&self) -> Vec<BlockId> {
        std::iter::once(self.header)
            .chain(self.chain.iter().copied())
            .collect()
    }
}
