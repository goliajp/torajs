//! Which `any` cells of a loop hold a number all the way round, and
//! which part of the loop can run with them unboxed.
//!
//! A web is a set of the loop's `any` cells joined by `copy`: the
//! accumulator and the result cell of each `+` that feeds it. Every
//! def of a web cell is classified ([`LaneDef`]): a box of a number
//! (the lanes are read off its operand), a copy of another web cell,
//! or opaque — a call's answer, a load. Every use is one the lanes
//! can answer (`any_is_int32`, `any_is_number`, `any_to_int32`,
//! `any_to_f64`, the refcount no-ops, a copy into the web) or not.
//!
//! The blocks with an opaque def or an unanswerable use are the
//! loop's slow part. The fast part is what the header reaches without
//! them, where an `any_is_number` of a web cell only takes its true
//! edge; the edges leaving the fast part for the slow part become
//! side exits. The plan stands when the fast part carries a back
//! edge and boxes a sum, nothing it defines is read past the loop
//! except through cells, and every reader after the loop can be told
//! from one before it.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{
    BlockId, DOUBLE_ENCODE_OFFSET, Function, InstKind, Operand, TAG_TYPE_NUMBER, Terminator, Type,
    ValueId,
};

use crate::float_demote::cfg::{successors, term_operand_value};
use crate::int_entry::region::{cells_of, readers_after};
use crate::rc_peephole::visit_value_operands;

const FAST_INST_BUDGET: usize = 512;

/// How a web cell's def gives the lanes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum LaneDef {
    /// a copy of another web cell: every lane copies
    Lanes(ValueId),
    /// `box_int32 i`: the int lane is `i`
    Int32(Operand),
    /// `box_f64 f`: the double lane is `f`
    Double(Operand),
    /// `box_any 3` of I64 bits: the double lane reinterprets them
    DoubleBits(Operand),
    /// `box_any 2` of an I64: an int32 when it fits
    IntMaybe(Operand),
    /// a constant number
    Const {
        i: i64,
        f: f64,
        is_int: bool,
    },
    Opaque,
}

/// The single-def values of a function, by id.
pub(super) type Defs<'a> = HashMap<ValueId, &'a InstKind>;

pub(super) fn single_defs<'a>(func: &'a Function, cells: &HashSet<ValueId>) -> Defs<'a> {
    let mut defs: Defs<'_> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result.filter(|r| !cells.contains(r)) {
                defs.insert(r, &inst.kind);
            }
        }
    }
    defs
}

fn fits_int32(i: i64) -> bool {
    i64::from(i as i32) == i
}

/// A constant encoding read back, when it is a number.
fn const_lanes(bits: i64) -> LaneDef {
    let u = bits as u64;
    if u >> 48 == TAG_TYPE_NUMBER >> 48 {
        let i = i64::from(u as u32 as i32);
        return LaneDef::Const {
            i,
            f: i as f64,
            is_int: true,
        };
    }
    if u & TAG_TYPE_NUMBER != 0 {
        return LaneDef::Const {
            i: 0,
            f: f64::from_bits(u.wrapping_sub(DOUBLE_ENCODE_OFFSET)),
            is_int: false,
        };
    }
    LaneDef::Opaque
}

/// The lanes the source of a `copy any` gives.
pub(super) fn classify(
    func: &Function,
    defs: &Defs<'_>,
    web: &HashSet<ValueId>,
    op: Operand,
) -> LaneDef {
    let Operand::Value(v) = op else {
        return LaneDef::Opaque;
    };
    if web.contains(&v) {
        return LaneDef::Lanes(v);
    }
    match defs.get(&v) {
        Some(InstKind::BoxInt32(i)) => LaneDef::Int32(*i),
        Some(InstKind::BoxF64(f)) => LaneDef::Double(*f),
        Some(InstKind::BoxAny(2, Operand::ConstI64(c))) => LaneDef::Const {
            i: *c,
            f: *c as f64,
            is_int: fits_int32(*c),
        },
        Some(InstKind::BoxAny(2, Operand::Value(w))) if func.value_type(*w) == Type::I64 => {
            LaneDef::IntMaybe(Operand::Value(*w))
        }
        Some(InstKind::BoxAny(3, Operand::ConstI64(bits))) => LaneDef::Const {
            i: 0,
            f: f64::from_bits(*bits as u64),
            is_int: false,
        },
        Some(InstKind::BoxAny(3, Operand::Value(w))) => match defs.get(w) {
            Some(InstKind::BitCastF64ToI64(f)) => LaneDef::Double(*f),
            _ => LaneDef::DoubleBits(Operand::Value(*w)),
        },
        Some(InstKind::IntToPtr(Operand::ConstI64(bits))) => const_lanes(*bits),
        _ => LaneDef::Opaque,
    }
}

/// A def that boxes a sum the loop computed.
fn boxes(def: LaneDef) -> bool {
    matches!(
        def,
        LaneDef::Int32(_) | LaneDef::Double(_) | LaneDef::DoubleBits(_) | LaneDef::IntMaybe(_)
    )
}

pub(super) struct Plan {
    pub(super) header: BlockId,
    /// the whole loop, sorted
    pub(super) region: Vec<BlockId>,
    /// the fast part, sorted
    pub(super) fast: Vec<BlockId>,
    /// the web, sorted
    pub(super) web: Vec<ValueId>,
    /// edges from the fast part into the slow part
    pub(super) cuts: Vec<(BlockId, BlockId)>,
    /// loop cells read after the loop, and the blocks reading them
    pub(super) out: Vec<ValueId>,
    pub(super) readers: Vec<BlockId>,
}

/// The web of `any` loop cells: seeded by every cell a box defines
/// inside the loop, closed over the copies among the loop's cells.
fn collect_web(
    func: &Function,
    l: &NaturalLoop,
    cells: &HashSet<ValueId>,
    defs: &Defs<'_>,
) -> HashSet<ValueId> {
    let any_cells: HashSet<ValueId> = cells_of(func, &l.body, cells)
        .into_iter()
        .filter(|c| func.value_type(*c) == Type::Any)
        .collect();
    let mut web: HashSet<ValueId> = HashSet::default();
    let copies: Vec<(ValueId, ValueId)> = l
        .body
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .filter_map(|inst| match (inst.result, &inst.kind) {
            (Some(r), InstKind::Copy(Type::Any, Operand::Value(v))) if any_cells.contains(&r) => {
                Some((r, *v))
            }
            _ => None,
        })
        .collect();
    for (r, v) in &copies {
        if any_cells.contains(v) {
            continue;
        }
        if boxes(classify(func, defs, &web, Operand::Value(*v))) {
            web.insert(*r);
        }
    }
    let mut changed = !web.is_empty();
    while changed {
        changed = false;
        for (r, v) in &copies {
            if any_cells.contains(v) && (web.contains(r) != web.contains(v)) {
                web.insert(*r);
                web.insert(*v);
                changed = true;
            }
        }
    }
    web
}

/// A use the lanes answer: the node reading a web cell, or a copy
/// into the web.
fn answerable(kind: &InstKind, result: Option<ValueId>, web: &HashSet<ValueId>) -> bool {
    match kind {
        InstKind::AnyIsInt32(_)
        | InstKind::AnyIsNumber(_)
        | InstKind::AnyToInt32(_)
        | InstKind::RcIncAny(_)
        | InstKind::RcDecAny(_) => true,
        InstKind::AnyToF64(Operand::Value(v), _) => web.contains(v),
        InstKind::Copy(Type::Any, _) => result.is_some_and(|r| web.contains(&r)),
        _ => false,
    }
}

/// The blocks of the loop the fast part must not contain, and the
/// `any_is_number` results it folds.
fn slow_blocks(
    func: &Function,
    l: &NaturalLoop,
    web: &HashSet<ValueId>,
    defs: &Defs<'_>,
) -> (HashSet<BlockId>, HashSet<ValueId>) {
    let mut slow: HashSet<BlockId> = HashSet::default();
    let mut folds: HashMap<ValueId, BlockId> = HashMap::default();
    for &b in &l.body {
        let block = &func.blocks[b.0 as usize];
        for inst in &block.insts {
            let mut touches = false;
            visit_value_operands(&inst.kind, |v| touches |= web.contains(&v));
            // a def of a web cell other than a copy (a call's answer
            // stored straight into the cell) is as opaque as a copy of
            // one
            let opaque_def = match (inst.result, &inst.kind) {
                (Some(r), InstKind::Copy(Type::Any, src)) if web.contains(&r) => {
                    classify(func, defs, web, *src) == LaneDef::Opaque
                }
                (Some(r), _) => web.contains(&r),
                (None, _) => false,
            };
            if opaque_def {
                slow.insert(b);
            }
            if touches && !answerable(&inst.kind, inst.result, web) {
                slow.insert(b);
            }
            if let (Some(r), InstKind::AnyIsNumber(Operand::Value(v))) = (inst.result, &inst.kind)
                && web.contains(v)
            {
                folds.insert(r, b);
            }
        }
        if term_operand_value(&block.term).is_some_and(|v| web.contains(&v)) {
            slow.insert(b);
        }
    }
    // a fold's answer may only decide a branch
    for &b in &l.body {
        for inst in &func.blocks[b.0 as usize].insts {
            visit_value_operands(&inst.kind, |v| {
                if let Some(at) = folds.remove(&v) {
                    slow.insert(at);
                }
            });
        }
    }
    (slow, folds.into_keys().collect())
}

/// The successors the fast part follows: a folded `any_is_number`
/// takes only its true edge.
fn fast_successors(term: &Terminator, folds: &HashSet<ValueId>) -> Vec<BlockId> {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            ..
        } if folds.contains(c) => vec![*then_blk],
        term => successors(term).collect(),
    }
}

/// What the header reaches inside the loop without a slow block.
fn fast_part(
    func: &Function,
    l: &NaturalLoop,
    slow: &HashSet<BlockId>,
    folds: &HashSet<ValueId>,
) -> Vec<BlockId> {
    let mut seen: HashSet<BlockId> = HashSet::default();
    let mut work = vec![l.header];
    while let Some(b) = work.pop() {
        if !l.contains(&b) || slow.contains(&b) || !seen.insert(b) {
            continue;
        }
        work.extend(fast_successors(&func.blocks[b.0 as usize].term, folds));
    }
    let mut fast: Vec<BlockId> = seen.into_iter().collect();
    fast.sort_by_key(|b| b.0);
    fast
}

/// What the planner found for a loop.
pub(super) enum Verdict {
    NoWeb,
    Skipped,
    Plan(Plan),
}

/// A single-def value the fast part defines and a block outside the
/// loop reads: only one of the two copies defines it under that id.
/// A read in the slow part is fine — a side exit writes it back.
fn escapes_loop(
    func: &Function,
    l: &NaturalLoop,
    fast: &HashSet<BlockId>,
    cells: &HashSet<ValueId>,
) -> bool {
    let mut own: HashSet<ValueId> = HashSet::default();
    for b in fast {
        for inst in &func.blocks[b.0 as usize].insts {
            own.extend(inst.result.filter(|r| !cells.contains(r)));
        }
    }
    func.blocks.iter().filter(|b| !l.contains(&b.id)).any(|b| {
        let mut hit = term_operand_value(&b.term).is_some_and(|v| own.contains(&v));
        for inst in &b.insts {
            visit_value_operands(&inst.kind, |v| hit |= own.contains(&v));
        }
        hit
    })
}

/// Whether the fast part reads a web cell's value: a copy whose lanes
/// nothing reads only boxes them again on the way out, more code with
/// nothing kept in a register.
fn reads_lane(func: &Function, fast: &[BlockId], web: &HashSet<ValueId>) -> bool {
    fast.iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .any(|inst| match &inst.kind {
            InstKind::AnyIsInt32(Operand::Value(v))
            | InstKind::AnyToInt32(Operand::Value(v))
            | InstKind::AnyToF64(Operand::Value(v), _) => web.contains(v),
            _ => false,
        })
}

pub(super) fn plan_loop(
    func: &Function,
    l: &NaturalLoop,
    dom: &DominatorTree,
    cells: &HashSet<ValueId>,
    defs: &Defs<'_>,
) -> Verdict {
    let web = collect_web(func, l, cells, defs);
    if web.is_empty() {
        return Verdict::NoWeb;
    }
    match close(func, l, dom, cells, defs, web) {
        Some(plan) => Verdict::Plan(plan),
        None => Verdict::Skipped,
    }
}

fn close(
    func: &Function,
    l: &NaturalLoop,
    dom: &DominatorTree,
    cells: &HashSet<ValueId>,
    defs: &Defs<'_>,
    mut web: HashSet<ValueId>,
) -> Option<Plan> {
    // a web cell only the slow part defines has no def to give its
    // lanes a value in the fast copy: it leaves the web, and a copy
    // from it into the web becomes an opaque def; repeat until every
    // web cell has a def in the fast part
    let (fast, folds) = loop {
        let (slow, folds) = slow_blocks(func, l, &web, defs);
        let fast = fast_part(func, l, &slow, &folds);
        let defined: HashSet<ValueId> = fast
            .iter()
            .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
            .filter_map(|inst| inst.result.filter(|r| web.contains(r)))
            .collect();
        if defined.len() == web.len() {
            break (fast, folds);
        }
        web = defined;
        if web.is_empty() {
            return None;
        }
    };
    let fast_set: HashSet<BlockId> = fast.iter().copied().collect();
    if !l.back_edge_sources.iter().any(|b| fast_set.contains(b)) {
        return None;
    }
    let size: usize = fast
        .iter()
        .map(|b| func.blocks[b.0 as usize].insts.len())
        .sum();
    if size > FAST_INST_BUDGET {
        return None;
    }
    let sums = fast
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .any(|inst| match (inst.result, &inst.kind) {
            (Some(r), InstKind::Copy(Type::Any, src)) if web.contains(&r) => {
                boxes(classify(func, defs, &web, *src))
            }
            _ => false,
        });
    if !sums || !reads_lane(func, &fast, &web) || escapes_loop(func, l, &fast_set, cells) {
        return None;
    }
    let (out, readers) = readers_after(func, &l.body, l.header, dom, cells)?;
    let mut cuts: Vec<(BlockId, BlockId)> = Vec::new();
    for &b in &fast {
        for s in fast_successors(&func.blocks[b.0 as usize].term, &folds) {
            if l.contains(&s) && !fast_set.contains(&s) {
                cuts.push((b, s));
            }
        }
    }
    let mut web: Vec<ValueId> = web.into_iter().collect();
    web.sort_by_key(|v| v.0);
    Some(Plan {
        header: l.header,
        region: l.body.clone(),
        fast,
        web,
        cuts,
        out,
        readers,
    })
}
