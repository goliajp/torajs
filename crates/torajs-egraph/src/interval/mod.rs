//! Integer range analysis — function-local interval lattice with
//! branch refinement and loop widening/narrowing (Cousot & Cousot
//! abstract interpretation; the lightweight analogue of LLVM
//! LVI/SCCP and the type/range half of TurboFan's representation
//! selection). Analysis only: no IR is mutated here.
//!
//! Clients (L3a item 10, RFC 20260611-int-range-float-demotion):
//!
//! * `sext_elide` R2 facts — any i64 value provably inside i32 range
//!   equals its own low-32 sign extension, which lets loop-carried
//!   cells (multi-def Copy, conservatively unknown to sext_elide's
//!   own one-pass lattice) qualify and collapses the residual
//!   popcount sext pair.
//! * `float_demote` (phase 1b) — integer-valuedness + bounds of f64
//!   hot-loop SCCs decide demotion and guard placement.
//!
//! Flow-insensitive per-value facts plus dominating-edge constraints:
//! a loop cell's fact is the join over its def sites, each evaluated
//! in its block's refined context; widening kicks in after a few
//! Kleene rounds so unbounded trajectories (collatz `3n+1`) settle at
//! a sentinel bound while keeping integer-valuedness, and a narrowing
//! sweep re-tightens bounds the refinement can recover (the
//! `i <= 1e6` loop-counter shape).

mod cache;
mod def_shapes;
mod edge;
pub(crate) mod elem;
mod elem_alias;
pub(crate) mod elem_header;
mod elem_kernels;
pub(crate) mod elem_len;
mod horizon;
mod live;
mod lookup;
mod memo;
mod merge;
pub mod refine;
mod refine_logic;
pub mod run;
pub mod square;
mod state;
pub mod transfer;
mod walk;
mod widen;

pub use cache::{FactCache, Seed, module_names};
pub use def_shapes::SingleDefs;
pub use transfer::{AbsVal, NumFact};

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Function, Module, Type, ValueId};

use def_shapes::def_shapes;
use live::Live;
use lookup::lookup;
use memo::Memo;
use refine::Constraints;
use state::State;
use torajs_core::ssa::dominator::DominatorTree;
use walk::Walk;
use widen::step;

/// Ascending rounds that widen through the thresholds; past it a
/// growing bound jumps straight to its sentinel, so the ascent still
/// reaches a post-fixpoint a few rounds later. Stopping the ascent here
/// instead is unsound: a cell that did not move in the last round can
/// hold a fact computed from a neighbour's older, smaller one, and
/// narrowing never grows it back. Sized so threshold widening (one
/// ladder step per round) can climb a realistic icmp-constant ladder
/// first.
const MAX_ASCEND: usize = 32;
/// Narrowing sweeps after the ascend stabilizes.
const NARROW_ROUNDS: usize = 2;

/// Analyze every function body. Index-aligned with `module.funcs`.
pub fn analyze_module(module: &Module) -> Vec<HashMap<ValueId, NumFact>> {
    let names: Vec<String> = module.funcs.iter().map(|f| f.name.clone()).collect();
    analyze_module_with(module, &names)
}

/// [`analyze_module`] under `names`, the module's function names.
pub fn analyze_module_with(module: &Module, names: &[String]) -> Vec<HashMap<ValueId, NumFact>> {
    module
        .funcs
        .iter()
        .map(|f| {
            if f.is_declaration() {
                HashMap::default()
            } else {
                analyze_function_with(f, names)
            }
        })
        .collect()
}

/// The numeric values whose fact proves them inside i32: an i64 one
/// equals its own sign-extended low 32 bits, an f64 one is
/// integer-valued and truncates to an i32 exactly.
pub fn sext32_set(facts: &HashMap<ValueId, NumFact>, func: &Function) -> HashSet<ValueId> {
    facts
        .iter()
        .filter(|(v, f)| {
            matches!(func.values[v.0 as usize].ty, Type::I64 | Type::F64) && f.is_sext32()
        })
        .map(|(v, _)| *v)
        .collect()
}

/// Analyze one function with no way to name its callees, so no array
/// element point is tracked — the facts are the same ones this pass
/// gave before [`elem`] existed, and still sound.
pub fn analyze_function(func: &Function) -> HashMap<ValueId, NumFact> {
    analyze_function_with(func, &[])
}

/// `callee_names` is index-aligned with `Module::funcs`; it is what
/// lets [`elem`] recognize the array runtime by name.
pub fn analyze_function_with(
    func: &Function,
    callee_names: &[String],
) -> HashMap<ValueId, NumFact> {
    let (single, cells, block_defs) = def_shapes(func);
    let dom = DominatorTree::compute(func);
    let constraints = Constraints::compute(func, &single);
    let (merges, inherited) = refine::scope::inherited(func, &dom, constraints, &block_defs);
    let (horizon, thresholds) = horizon::Horizon::new(func, &single, &cells);
    // An allocation's elements are one more multi-def cell whose defs
    // happen to be stores; it rides the same ascent below.
    let elems = elem::collect(func, callee_names);
    let mut elem_state: HashMap<ValueId, AbsVal> = HashMap::default();

    let mut state = State::new(func.values.len());
    for p in &func.params {
        state.insert(*p, opaque_for(func.values[p.0 as usize].ty));
    }

    // each cell's slot in the per-round join, a flat table
    let mut cell_ids: Vec<ValueId> = cells.iter().copied().collect();
    cell_ids.sort_unstable_by_key(|v| v.0);
    let mut slot = vec![u32::MAX; func.values.len()];
    for (k, c) in cell_ids.iter().enumerate() {
        slot[c.0 as usize] = k as u32;
    }
    // per cell, the join of the defs this round evaluated (none for a
    // cell no reached block writes)
    let mut contrib: Vec<Option<AbsVal>> = vec![None; cell_ids.len()];

    let mut live = Live::new(&inherited, func.values.len());
    let mut memo = Memo::new(
        func,
        &inherited,
        &merges,
        |v| slot[v.0 as usize] != u32::MAX,
        |v| elems.reads.contains_key(&v),
    );
    let squares = inherited.square_blocks();
    let mut stable_since = None;
    // the merge ranges, indexed like the joins of `merges`
    let mut merged: Vec<Option<NumFact>> = Vec::new();
    // the roots of the square bounds the previous round read
    let mut roots: Vec<i128> = Vec::new();
    for round in 0.. {
        let narrowing = stable_since.is_some();
        let with_roots;
        let thresholds: &[i128] = if round >= MAX_ASCEND {
            &[]
        } else if roots.is_empty() {
            &thresholds
        } else {
            with_roots = square::with_roots(&thresholds, &roots);
            &with_roots
        };
        roots.clear();
        memo.read_merges(&merges, &state, &inherited, &mut merged);
        let mut changed = false;

        let mut walk = Walk {
            func,
            slot: &slot,
            squares: &squares,
            elems: &elems,
            elem_state: &elem_state,
            merged: &merged,
            state: &mut state,
            contrib: &mut contrib,
            live: &mut live,
            memo: &mut memo,
            roots: &mut roots,
        };
        for &bid in dom.reverse_postorder() {
            changed |= walk.block(bid);
        }

        for (root, ws) in &elems.writes {
            let new = ws
                .iter()
                .fold(AbsVal::Bottom, |acc, op| acc.join(lookup(op, &state, &[])));
            let old = elem_state.get(root).copied().unwrap_or(AbsVal::Bottom);
            let next = step(old, new, narrowing, round, thresholds);
            if next != old {
                elem_state.insert(*root, next);
                memo.elems_moved();
                changed = true;
            }
        }
        // every cell steps from the same state: a counter's cap reads
        // its init cells, so updating in place would let map order
        // decide which of them it sees
        let stepped: Vec<(ValueId, AbsVal)> = contrib
            .iter_mut()
            .zip(&cell_ids)
            .filter_map(|(c, &cell)| {
                let new = c.take()?;
                let old = state.get(&cell).copied().unwrap_or(AbsVal::Bottom);
                let next = step(old, new, narrowing, round, thresholds);
                let capped = horizon.cap(cell, next, &state);
                // past the threshold budget a counter whose cap still
                // moves takes the uncapped bound: the cap follows an init
                // that is itself growing, one horizon a round, and two
                // counters seeding each other would climb toward the end
                // of i64 that way for thousands of rounds
                let next = if narrowing || round < MAX_ASCEND || capped == old {
                    capped
                } else {
                    next
                };
                (next != old).then_some((cell, next))
            })
            .collect();
        for (cell, next) in stepped {
            state.insert(cell, next);
            memo.moved(cell);
            changed = true;
        }

        match (narrowing, changed) {
            (false, false) => stable_since = Some(round),
            (true, _) if round >= stable_since.unwrap() + NARROW_ROUNDS => break,
            _ => {}
        }
    }

    state.into_facts()
}

fn opaque_for(ty: Type) -> AbsVal {
    match ty {
        Type::I64 => AbsVal::Fact(NumFact::full_i64()),
        _ => AbsVal::Top,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FPred, IPred, Inst, InstKind, Operand, Terminator, ValueInfo,
    };

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn func(values: Vec<Type>, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values: values
                .into_iter()
                .map(|ty| ValueInfo { ty, name: None })
                .collect(),
            current_origin: None,
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    #[test]
    fn popcount_counter_cell_proves_sext32() {
        // bb0: %0 = copy 0            ; i counter cell
        // bb1: %1 = icmp slt %0, 1e7 ; cond_br %1, bb2, bb4
        // bb2: %2 = copy %0           ; n cell init
        //      %3 = sub %2, 1
        //      %4 = and %2, %3
        //      %2 = copy %4           ; n cell loop def
        //      %5 = add %0, 1
        //      %0 = copy %5           ; counter loop def
        //      br bb1
        // bb4: ret
        let f = func(
            vec![Type::I64; 6],
            vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(Type::I64, c(0)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::ICmp(IPred::Slt, v(0), c(10_000_000)))],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(2, InstKind::Copy(Type::I64, v(0))),
                        inst(3, InstKind::BinOp(BinOp::Sub, v(2), c(1))),
                        inst(4, InstKind::BinOp(BinOp::And, v(2), v(3))),
                        inst(2, InstKind::Copy(Type::I64, v(4))),
                        inst(5, InstKind::BinOp(BinOp::Add, v(0), c(1))),
                        inst(0, InstKind::Copy(Type::I64, v(5))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(None)),
            ],
        );
        let facts = analyze_function(&f);
        // counter cell: [0, 1e7] (refined increment joins the init)
        assert_eq!(facts[&ValueId(0)], NumFact::new(0, 10_000_000));
        // n cell and the and-result stay inside i32 → sext_elide seeds
        let seeds = sext32_set(&facts, &f);
        assert!(seeds.contains(&ValueId(2)), "n cell: {facts:?}");
        assert!(seeds.contains(&ValueId(4)), "and result: {facts:?}");
    }

    #[test]
    fn a_def_off_the_dominator_chain_ends_a_bound() {
        // bb0: %0 = copy 0                        ; d
        // bb1: %1 = icmp slt %0, 10 ; cond_br %1, bb2, bb5
        // bb2: %2 = icmp eq %0, 5   ; cond_br %2, bb3, bb4
        // bb3: %0 = copy 1000       ; br bb4      (off bb4's idom chain)
        // bb4: %3 = add %0, 1 ; %0 = copy %3 ; br bb1
        // bb5: ret
        let cbr = |c: u32, t: u32, e: u32| Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        };
        let f = func(
            vec![Type::I64, Type::Bool, Type::Bool, Type::I64],
            vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(Type::I64, c(0)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::ICmp(IPred::Slt, v(0), c(10)))],
                    cbr(1, 2, 5),
                ),
                block(
                    2,
                    vec![inst(2, InstKind::ICmp(IPred::Eq, v(0), c(5)))],
                    cbr(2, 3, 4),
                ),
                block(
                    3,
                    vec![inst(0, InstKind::Copy(Type::I64, c(1000)))],
                    Terminator::Br(BlockId(4)),
                ),
                block(
                    4,
                    vec![
                        inst(3, InstKind::BinOp(BinOp::Add, v(0), c(1))),
                        inst(0, InstKind::Copy(Type::I64, v(3))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(5, vec![], Terminator::Ret(None)),
            ],
        );
        let facts = analyze_function(&f);
        assert!(
            facts.get(&ValueId(3)).is_none_or(|x| x.hi >= 1001),
            "d + 1 past the arm: {facts:?}"
        );
    }

    #[test]
    fn a_def_around_a_loop_ends_a_bound_from_above_it() {
        // bb0: %0 = copy 2                        ; d
        // bb1: %1 = icmp slt %0, 5  ; cond_br %1, bb2, bb5
        // bb2: %4 = copy 0          ; br bb3      ; t
        // bb3: %5 = icmp slt %4, 4  ; cond_br %5, bb4, bb5
        // bb4: %3 = add %0, 1 ; %6 = mul %0, 7 ; %0 = copy %6
        //      %7 = add %4, 1 ; %4 = copy %7 ; br bb3
        // bb5: ret
        let cbr = |c: u32, t: u32, e: u32| Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        };
        let i64_ = Type::I64;
        let f = func(
            vec![
                i64_,
                Type::Bool,
                Type::Bool,
                i64_,
                i64_,
                Type::Bool,
                i64_,
                i64_,
            ],
            vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(i64_, c(2)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::ICmp(IPred::Slt, v(0), c(5)))],
                    cbr(1, 2, 5),
                ),
                block(
                    2,
                    vec![inst(4, InstKind::Copy(i64_, c(0)))],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    3,
                    vec![inst(5, InstKind::ICmp(IPred::Slt, v(4), c(4)))],
                    cbr(5, 4, 5),
                ),
                block(
                    4,
                    vec![
                        inst(3, InstKind::BinOp(BinOp::Add, v(0), c(1))),
                        inst(6, InstKind::BinOp(BinOp::Mul, v(0), c(7))),
                        inst(0, InstKind::Copy(i64_, v(6))),
                        inst(7, InstKind::BinOp(BinOp::Add, v(4), c(1))),
                        inst(4, InstKind::Copy(i64_, v(7))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                block(5, vec![], Terminator::Ret(None)),
            ],
        );
        let facts = analyze_function(&f);
        assert!(
            facts.get(&ValueId(3)).is_none_or(|x| x.hi >= 15),
            "d + 1 after the first trip: {facts:?}"
        );
    }

    #[test]
    fn collatz_f64_cell_stays_integer_valued() {
        // bb0: %0 = copy 1                 ; i cell (i64)
        // bb1: %1 = icmp sle %0, 1000000  ; cond_br bb2, bb8
        // bb2: %2 = sitofp %0
        //      %3 = copy f64 %2            ; n cell init
        //      br bb3
        // bb3: %4 = frem %3, 2
        //      %5 = fcmp oeq %4, 0 ; cond_br bb4, bb5
        // bb4: %6 = fdiv %3, 2 ; %3 = copy %6 ; br bb6
        // bb5: %7 = fmul 3, %3 ; %8 = fadd %7, 1 ; %3 = copy %8 ; br bb6
        // bb6: br bb3   (loop back; exit path elided — analysis only)
        let two = Operand::ConstF64(2.0);
        let f = func(
            vec![
                Type::I64,
                Type::Bool,
                Type::F64,
                Type::F64,
                Type::F64,
                Type::Bool,
                Type::F64,
                Type::F64,
                Type::F64,
            ],
            vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(Type::I64, c(1)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::ICmp(IPred::Sle, v(0), c(1_000_000)))],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(2),
                        else_blk: BlockId(7),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(2, InstKind::SiToFp(v(0))),
                        inst(3, InstKind::Copy(Type::F64, v(2))),
                    ],
                    Terminator::Br(BlockId(3)),
                ),
                block(
                    3,
                    vec![
                        inst(4, InstKind::BinOp(BinOp::FRem, v(3), two)),
                        inst(5, InstKind::FCmp(FPred::Oeq, v(4), Operand::ConstF64(0.0))),
                    ],
                    Terminator::CondBr {
                        cond: v(5),
                        then_blk: BlockId(4),
                        else_blk: BlockId(5),
                    },
                ),
                block(
                    4,
                    vec![
                        inst(6, InstKind::BinOp(BinOp::FDiv, v(3), two)),
                        inst(3, InstKind::Copy(Type::F64, v(6))),
                    ],
                    Terminator::Br(BlockId(6)),
                ),
                block(
                    5,
                    vec![
                        inst(
                            7,
                            InstKind::BinOp(BinOp::FMul, Operand::ConstF64(3.0), v(3)),
                        ),
                        inst(
                            8,
                            InstKind::BinOp(BinOp::FAdd, v(7), Operand::ConstF64(1.0)),
                        ),
                        inst(3, InstKind::Copy(Type::F64, v(8))),
                    ],
                    Terminator::Br(BlockId(6)),
                ),
                block(6, vec![], Terminator::Br(BlockId(3))),
                block(7, vec![], Terminator::Ret(None)),
            ],
        );
        let facts = analyze_function(&f);
        // the n cell is integer-valued (has a fact at all) even though
        // its upper bound widens to the sentinel — the 1b demotion
        // contract: int-ness provable, bound guarded at runtime.
        let n = facts[&ValueId(3)];
        assert!(n.lo >= 0, "n cell fact: {n:?}");
        assert!(n.hi >= transfer::POS_INF, "3n+1 must widen: {n:?}");
        // the fdiv result is integer-valued thanks to the parity guard
        assert!(
            facts.contains_key(&ValueId(6)),
            "fdiv even-proof: {facts:?}"
        );
        // the frem result is bounded
        assert_eq!(facts[&ValueId(4)], NumFact::new(0, 1));
    }

    #[test]
    fn fdiv_without_parity_guard_is_unknown() {
        // straight-line fdiv by 2 with no frem branch — must stay Top.
        let f = func(
            vec![Type::I64, Type::F64, Type::F64],
            vec![block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::I64, c(5))),
                    inst(1, InstKind::SiToFp(v(0))),
                    inst(
                        2,
                        InstKind::BinOp(BinOp::FDiv, v(1), Operand::ConstF64(2.0)),
                    ),
                ],
                Terminator::Ret(None),
            )],
        );
        let facts = analyze_function(&f);
        assert!(!facts.contains_key(&ValueId(2)));
        assert_eq!(facts[&ValueId(1)], NumFact::point(5));
    }

    #[test]
    fn refinement_requires_sole_predecessor() {
        // bb1 is reached both from the branch and from bb2 — the
        // icmp constraint must NOT attach.
        let f = func(
            vec![Type::I64, Type::Bool],
            vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, c(0))),
                        inst(1, InstKind::ICmp(IPred::Slt, v(0), c(10))),
                    ],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                block(1, vec![], Terminator::Ret(None)),
                block(2, vec![], Terminator::Br(BlockId(1))),
            ],
        );
        // would panic on inconsistency if constraints attached wrongly;
        // the cell-free function must still produce the plain fact.
        let facts = analyze_function(&f);
        assert_eq!(facts[&ValueId(0)], NumFact::point(0));
    }
}
