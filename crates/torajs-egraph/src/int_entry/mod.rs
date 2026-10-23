//! Loop versioning on an integer bound — for a loop whose counter is
//! an i64 and whose bound is an f64 nothing is known of: a size that
//! arrived at run time (`parseInt(process.argv[2], 10)`), a
//! parameter, a load.
//!
//! The header of such a loop reads `fcmp olt (sitofp i), n`, and the
//! interval analysis learns nothing from it: `n` has no fact, so `i`
//! has no upper bound, and every consumer of bounds downstream —
//! float demotion, the sign-extension seeds, the popcount idiom, the
//! growth-guard fold — stands down. With a literal bound the same
//! loop was bounded; the source only moved its size to the command
//! line.
//!
//! The loop is copied ([`region`]). In front of the two a chain of
//! checks rounds the bound to an integer once ([`guard`]); the copy
//! it passes into compares `i` with that integer, and what it fails
//! into is the loop as it was. Nothing is assumed of the bound: NaN,
//! an infinity, anything at or past 2^53 takes the loop as it was.
//!
//! A value the loop reads — the modulus of `i % m`, a bound it also
//! adds — goes through the same chain: it passes when it is an
//! integer inside the window its readers ask for, and the copy reads
//! it back as the `sitofp` of that integer, which the interval
//! analysis has a fact for. A divisor's window starts at 1, so the
//! `%` the copy does is the integer one.
//!
//! A loop whose bound informs nothing but its exit tests ([`demand`]) is
//! not copied: an integer test that implies the f64 one goes in front
//! of each that leaves every loop around it ([`exit_test`]).
//!
//! Runs after `frem_narrow` — whose truncation rule does not check
//! 2^53 and must not be fed the `sitofp`s minted here — and before
//! the interval analysis, which reads the assumption back out of the
//! checks. `TORAJS_INT_ENTRY_OFF=1` skips it.

mod demand;
pub(crate) mod exit_split;
pub(crate) mod exit_test;
pub(crate) mod guard;
mod order;
pub(crate) mod region;
mod sources;
pub(crate) mod trim;
mod window;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{BinOp, BlockId, Function, IPred, InstKind, Module, Operand, Type, ValueId};

use crate::float_demote::cfg::{multi_def_values, remap_term_operand, retarget_edge};
use crate::slot_forward::rewrite_operands;
use guard::{Need, Round};
use sources::{bound_uses, is_source, value_sources};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IntEntryStats {
    pub loops_versioned: u32,
    /// f64 comparisons the fast copies make between integers
    pub bounds_converted: u32,
    /// f64 values the fast copies read as the image of an integer
    pub values_converted: u32,
    /// loops left alone: a value they define is read after them, a
    /// cell of theirs by code that may also run before them, or no
    /// order of the plans puts theirs before the loops it must
    /// precede (see `order`)
    pub skipped_escaping: u32,
    /// loops left alone: too large to copy
    pub skipped_budget: u32,
    /// loops given an integer exit test instead of a copy
    pub exit_tests: u32,
}

const REGION_INST_BUDGET: usize = 512;
const FUNCTION_INST_BUDGET: usize = 2000;

/// The widest window. Both `sitofp` and the comparison are exact up
/// to 2^53 - 1, but the counter's cell also holds the value that
/// ended the loop, one step past the bound, and a cell whose fact
/// touches 2^53 is not one float demotion takes for exact. Half the
/// range leaves any step room.
const EXACT_MAX: i64 = 1 << 52;
/// The widest window for a value the loop reads: `sitofp` is exact
/// up to here, and a fact ending here still counts as exact.
const EXACT_VALUE_MAX: i64 = (1 << 53) - 1;
/// What the counter of a loop that does 32-bit work must stay inside
/// for that work to be provably 32-bit.
const INT32_MAX: i64 = i32::MAX as i64;
const INT32_MIN: i64 = i32::MIN as i64;

/// One `fcmp` between the counter's image and an opaque bound.
struct BoundUse {
    block: BlockId,
    inst: usize,
    source: ValueId,
    counter: Operand,
    pred: IPred,
    round: Round,
}

struct Plan {
    header: BlockId,
    region: Vec<BlockId>,
    uses: Vec<BoundUse>,
    /// sources the loop reads as values, or starts a cell from
    values: Vec<ValueId>,
    needs: Vec<Need>,
    /// loop cells read after the loop, and the blocks reading them
    out: Vec<ValueId>,
    readers: Vec<BlockId>,
}

/// A loop given a fast copy, as the pipeline tail finds it again: by
/// the conditions of its checks and the values its chain minted, ids
/// that hold while the blocks are renumbered.
pub(crate) struct Versioned {
    pub(crate) func: usize,
    /// in chain order, with whether the slow loop is the `then` side
    pub(crate) checks: Vec<(ValueId, bool)>,
    pub(crate) minted: std::ops::Range<u32>,
    /// the slow loop's f64 tests against the bound
    pub(crate) tests: Vec<exit_test::TestAt>,
}

/// The versioning's counters and the loops it copied.
pub struct Versioning {
    pub stats: IntEntryStats,
    pub(crate) loops: Vec<Versioned>,
}

impl std::fmt::Debug for Versioning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.stats.fmt(f)
    }
}

pub fn version_loops(module: &mut Module) -> IntEntryStats {
    version_loops_recorded(module).stats
}

pub fn version_loops_recorded(module: &mut Module) -> Versioning {
    let mut stats = IntEntryStats::default();
    let mut loops = Vec::new();
    for (fi, func) in module.funcs.iter_mut().enumerate() {
        let dense = func
            .blocks
            .iter()
            .enumerate()
            .all(|(i, b)| b.id.0 as usize == i);
        // every bound and value a plan converts is an f64
        let f64s = func.values.iter().any(|v| v.ty == Type::F64);
        if func.is_declaration() || !dense || !f64s {
            continue;
        }
        let (plans, tests) = plan_function(func, &mut stats);
        let (plans, cyclic) = order::applying_order(func, plans);
        stats.skipped_escaping += cyclic;
        for plan in plans {
            let tests = plan
                .uses
                .iter()
                .filter_map(|u| {
                    let test = func.blocks[u.block.0 as usize].insts[u.inst].result?;
                    Some(exit_test::TestAt {
                        test,
                        source: u.source,
                        round: u.round,
                        pred: u.pred,
                    })
                })
                .collect();
            let (checks, minted) = apply(func, &plan);
            loops.push(Versioned {
                func: fi,
                checks,
                minted,
                tests,
            });
            stats.loops_versioned += 1;
            stats.bounds_converted += plan.uses.len() as u32;
            stats.values_converted += plan.values.len() as u32;
        }
        // after the copies: they only append blocks and rename reads
        for test in &tests {
            exit_test::apply(func, test);
            stats.exit_tests += 1;
        }
    }
    Versioning { stats, loops }
}

fn plan_function(
    func: &Function,
    stats: &mut IntEntryStats,
) -> (Vec<Plan>, Vec<exit_test::ExitTest>) {
    let dom = DominatorTree::compute(func);
    let loops = LoopAnalysis::compute(func, &dom);
    let mut outermost_first: Vec<_> = loops.loops().iter().collect();
    outermost_first.sort_by_key(|l| (std::cmp::Reverse(l.body.len()), l.header.0));

    let cells = multi_def_values(func);
    let mut defs: HashMap<ValueId, (BlockId, &InstKind)> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result.filter(|r| !cells.contains(r)) {
                defs.insert(r, (block.id, &inst.kind));
            }
        }
    }

    let mut plans = Vec::new();
    let mut tests = Vec::new();
    let mut taken: HashSet<BlockId> = HashSet::default();
    let (mut budget, mut reads) = (FUNCTION_INST_BUDGET, None);
    for l in outermost_first {
        if l.header.0 == 0 || l.body.iter().any(|b| taken.contains(b)) {
            continue;
        }
        let region: HashSet<BlockId> = l.body.iter().copied().collect();
        let mut uses = bound_uses(func, &region, &defs, &cells);
        let matched: HashSet<(BlockId, usize)> = uses.iter().map(|u| (u.block, u.inst)).collect();
        let mut values = value_sources(func, &region, &defs, &matched);
        // a cell every way into the loop starts from one source: the
        // loop reads the source through the cell
        let mut seeded: Vec<(ValueId, ValueId)> = Vec::new();
        for cell in region::cells_of(func, &l.body, &cells) {
            if let Some(Operand::Value(from)) =
                region::entry_value(func, &region, l.header, cell, &cells)
                && is_source(func, &region, &defs, from)
            {
                seeded.push((from, cell));
                if !values.contains(&from) {
                    values.push(from);
                }
            }
        }
        // a bound the loop also reads is taken as it is, not rounded
        for u in &mut uses {
            if values.contains(&u.source) {
                u.round = Round::Exact;
            }
        }
        if uses.is_empty() && values.is_empty() {
            continue;
        }
        // the integer tests are all a copy would buy: an exit test buys them
        if seeded.is_empty() && !demand::informs(func, &region, &uses, &values, &matched) {
            let reads = reads.get_or_insert_with(|| exit_test::reads_of(func));
            if let Some(test) = exit_test::plan(func, &loops, l.header, &region, &uses, reads) {
                taken.extend(region);
                tests.push(test);
                continue;
            }
        }
        let size: usize = l
            .body
            .iter()
            .map(|b| func.blocks[b.0 as usize].insts.len())
            .sum();
        if size > REGION_INST_BUDGET || size > budget {
            stats.skipped_budget += 1;
            continue;
        }
        let after = region::readers_after(func, &l.body, l.header, &dom, &cells);
        let Some((out, readers)) = after.filter(|_| !region::escapes(func, &region, &cells)) else {
            stats.skipped_escaping += 1;
            continue;
        };
        let int32 = does_int32_work(func, &region);
        let mut needs = needs_of(&uses, int32);
        for &source in &values {
            let mut starts = vec![source];
            starts.extend(seeded.iter().filter(|(s, _)| *s == source).map(|(_, c)| *c));
            let (lo, hi) = window::value_window(func, &region, &starts, int32);
            join_need(&mut needs, source, Round::Exact, lo, hi);
        }
        taken.extend(region);
        // a reader is rewritten once, for one loop
        taken.extend(readers.iter().copied());
        budget -= size;
        plans.push(Plan {
            header: l.header,
            region: l.body.clone(),
            uses,
            values,
            needs,
            out,
            readers,
        });
    }
    (plans, tests)
}

/// A sign-extension pair or a bitwise operator: the loop is doing
/// 32-bit work, and proving it so needs the counter inside int32.
fn does_int32_work(func: &Function, region: &HashSet<BlockId>) -> bool {
    region.iter().any(|b| {
        func.blocks[b.0 as usize].insts.iter().any(|inst| {
            matches!(
                inst.kind,
                InstKind::BinOp(
                    BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Shl | BinOp::AShr,
                    ..
                )
            )
        })
    })
}

/// One need per (source, rounding), its window the intersection of
/// what each comparison reading it asks for. A narrower window sends
/// more inputs to the slow copy; it never changes an answer.
fn needs_of(uses: &[BoundUse], int32: bool) -> Vec<Need> {
    let mut needs: Vec<Need> = Vec::new();
    for u in uses {
        let (lo, hi) = match (int32, u.pred) {
            (false, _) => (-EXACT_MAX, EXACT_MAX),
            (true, IPred::Slt) => (-EXACT_MAX, INT32_MAX + 1),
            (true, IPred::Sle) => (-EXACT_MAX, INT32_MAX),
            (true, IPred::Sge) => (INT32_MIN, EXACT_MAX),
            (true, _) => (INT32_MIN - 1, EXACT_MAX),
        };
        join_need(&mut needs, u.source, u.round, lo, hi);
    }
    needs
}

fn join_need(needs: &mut Vec<Need>, source: ValueId, round: Round, lo: i64, hi: i64) {
    match needs
        .iter_mut()
        .find(|n| n.source == source && n.round == round)
    {
        Some(n) => {
            n.lo = n.lo.max(lo);
            n.hi = n.hi.min(hi);
        }
        None => needs.push(Need {
            source,
            round,
            lo,
            hi,
        }),
    }
}

/// Answers the chain's checks and the ids of the values it minted.
fn apply(func: &mut Function, plan: &Plan) -> (Vec<(ValueId, bool)>, std::ops::Range<u32>) {
    let outside = func.blocks.len();
    let cells = multi_def_values(func);
    let copy = region::clone_blocks(func, &plan.region);
    let first = func.values.len() as u32;
    let chain = guard::build_chain(func, &plan.needs, plan.header, copy.blocks[&plan.header]);
    let minted = first..func.values.len() as u32;
    region::seed_cells(
        func,
        &plan.region,
        plan.header,
        outside,
        chain.pass,
        &cells,
        &copy,
    );
    // every way into the loop from outside it now enters the chain;
    // the back edges stay where they are
    let region: HashSet<BlockId> = plan.region.iter().copied().collect();
    for bi in 0..outside {
        if !region.contains(&BlockId(bi as u32)) {
            retarget_edge(&mut func.blocks[bi].term, plan.header, chain.head);
        }
    }
    region::write_out(func, &plan.region, &plan.out, &plan.readers, &copy);
    // the copy reads each value as the image of its integer
    let images: HashMap<ValueId, Operand> = plan
        .values
        .iter()
        .map(|s| (*s, Operand::Value(chain.values[s])))
        .collect();
    if !images.is_empty() {
        // the copy's cells start from the image too
        for inst in &mut func.blocks[chain.pass.0 as usize].insts {
            rewrite_operands(&mut inst.kind, &images);
        }
        for b in &plan.region {
            let block = &mut func.blocks[copy.blocks[b].0 as usize];
            for inst in &mut block.insts {
                rewrite_operands(&mut inst.kind, &images);
            }
            block.term = remap_term_operand(block.term.clone(), &images);
        }
    }
    for u in &plan.uses {
        let bound = Operand::Value(chain.rounded[&(u.source, u.round)]);
        let at = copy.blocks[&u.block];
        func.blocks[at.0 as usize].insts[u.inst].kind =
            InstKind::ICmp(u.pred, copy.operand(u.counter), bound);
    }
    (chain.checks, minted)
}

#[cfg(test)]
mod tests;
