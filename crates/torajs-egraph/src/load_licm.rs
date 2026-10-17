//! Array header-field load hoisting — the one family of `Load` the
//! egraph's LICM (`optimize_licm.rs`) may not move, because it has no
//! alias model: `for (i = 0; i < src.length; i++) dst.push(src[i])`
//! re-read `src`'s length and data pointer on every iteration.
//!
//! The alias model is the allocation-site one `interval::elem` already
//! keeps: a live tracked array is a fresh allocation whose address
//! never leaves the function's SSA, so the only instructions that can
//! touch its cell are the ones naming it. Its header — length (+8),
//! capacity/head (+16), props (+24), data pointer (+32) — is a
//! separate slice from its element slots (HotSpot C2's offset alias
//! slices, JSC B3's heap ranges): a slot write goes through the data
//! pointer at or past +40 and never lands in [0, 40).
//!
//! For each natural loop, innermost first, a `Load(_, p, 8 | 16 | 32)`
//! whose `p` names live array A and is defined outside the loop moves
//! to the tail of `idom(header)` when nothing in the loop body writes
//! A's header: no `Store` through a name of A, and no `Call` /
//! `CallIndirect` taking a name of A or of its data pointer (push,
//! reserve, shift, splice, a `length` write, a user function — all of
//! them call with A). The refcount / flags word (+0 / +4) is out of
//! scope as a word — rc helpers and the cycle collector write it — but
//! a length-lock test that reads only flags the collector never flips
//! (`lshr 48` then a mask) moves out whole.
//!
//! The moved load runs on paths that never enter the loop too. That
//! is safe: A comes from `__torajs_arr_alloc*`, which never answers
//! null and writes the whole header before returning, or is a frame
//! cell whose header lowering writes in the cell's own block, and the
//! cell never moves (a grow swaps the data buffer, not the cell); the only
//! instruction that frees it is a drop naming A, which the loop body
//! does not contain, and one before `idom(header)` would already be a
//! use-after-free in the loop itself.
//!
//! `TORAJS_LOAD_LICM_OFF=1` skips, `TORAJS_LOAD_LICM_STATS=1` counts.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, HEADER_FLAGS_COLLECTOR, Inst, InstKind, Module, Operand, Terminator,
    Type, ValueId, visit_value_operands,
};

use crate::interval::elem::{ArrayClasses, array_classes};
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

pub mod reread;

#[derive(Debug, Default)]
pub struct LoadLicmStats {
    /// Header-field loads moved out of a loop (a load hoisted through
    /// two nested loops counts twice).
    pub loads_hoisted: u32,
}

pub(crate) const ARR_LEN_OFF: u64 = 8;
const ARR_CAP_HEAD_OFF: u64 = 16;
pub(crate) const ARR_DATA_PTR_OFF: u64 = 32;

/// The pop's length-lock raiser reads the flags and length, and at
/// most deletes shadow entries inside the props dynobj; it writes no
/// header word.
const HEADER_READ_ONLY_CALLS: &[&str] = &[
    "__torajs_arr_delete_tail_guard",
    "__torajs_arr_species_guard",
];

pub fn hoist_header_loads(module: &mut Module, known: Option<&[String]>) -> LoadLicmStats {
    let names = crate::interval::module_names(known, module);
    let mut stats = LoadLicmStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        if let Some(classes) = array_classes(func, &names) {
            hoist_fn(func, &classes, &names, &mut stats);
        }
    }
    stats
}

fn hoist_fn(
    func: &mut Function,
    classes: &ArrayClasses,
    names: &[String],
    stats: &mut LoadLicmStats,
) {
    if classes.live.is_empty() {
        return;
    }
    let dom = DominatorTree::compute(func);
    let mut loops = LoopAnalysis::compute(func, &dom).loops().to_vec();
    // inner first: an inner loop's preheader is in the outer body, so
    // what moves out of the inner loop is judged again by the outer
    loops.sort_by_key(|l| l.body.len());
    let mut def_block: HashMap<ValueId, BlockId> = HashMap::default();
    for (bi, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                def_block.insert(r, BlockId(bi as u32));
            }
        }
    }
    let uses = use_counts(func);
    let n = func.blocks.len();
    for lp in &loops {
        let Some(pre) = dom.immediate_dominator(lp.header) else {
            continue;
        };
        if pre == lp.header || lp.contains(&pre) || pre.0 as usize >= n {
            continue;
        }
        let body: HashSet<BlockId> = lp.body.iter().copied().collect();
        if !sole_entry(func, pre, lp.header, &body) {
            continue;
        }
        let dirty = written_headers(func, &body, classes, names);
        let clean = |def_block: &HashMap<ValueId, BlockId>, p: &ValueId, offs: &[u64]| {
            classes.class.get(p).is_some_and(|a| {
                classes.live.contains(a)
                    && !dirty.contains(&(*a, None))
                    && offs.iter().all(|o| !dirty.contains(&(*a, Some(*o))))
            }) && def_block.get(p).is_some_and(|d| !body.contains(d))
        };
        for &b in &lp.body {
            let chains = flag_test_chains(&func.blocks[b.0 as usize].insts, &uses, |p| {
                clean(&def_block, p, &[0, 4])
            });
            let insts = std::mem::take(&mut func.blocks[b.0 as usize].insts);
            let (moved, kept): (Vec<_>, Vec<_>) = insts.into_iter().partition(|inst| {
                if inst.result.is_some_and(|r| chains.contains(&r)) {
                    return true;
                }
                let InstKind::Load(_, Operand::Value(p), off) = &inst.kind else {
                    return false;
                };
                matches!(*off, ARR_LEN_OFF | ARR_CAP_HEAD_OFF | ARR_DATA_PTR_OFF)
                    && clean(&def_block, p, &[*off])
            });
            func.blocks[b.0 as usize].insts = kept;
            for inst in moved {
                if let Some(r) = inst.result {
                    def_block.insert(r, pre);
                }
                func.blocks[pre.0 as usize].insts.push(inst);
                stats.loads_hoisted += 1;
            }
        }
    }
}

/// The header-word flag tests in `insts` that may leave the loop:
/// `t = load i64 A+0` masked either after `s = lshr t, 48` by
/// `k = and s, K`, or in place by `k = and t, K << 48`, each the only
/// use of the one before, in one block, where A's header is written by
/// nothing in the loop and K selects no bit the cycle collector flips
/// (`HEADER_FLAGS_COLLECTOR`) — every other flag moves only inside a
/// runtime call handed the array, and the refcount sits below bit 32.
fn flag_test_chains(
    insts: &[Inst],
    uses: &HashMap<ValueId, u32>,
    clean: impl Fn(&ValueId) -> bool,
) -> HashSet<ValueId> {
    let mut chains = HashSet::default();
    let single = |v: ValueId| uses.get(&v) == Some(&1);
    let flags_only = |k: u64| k & i64::from(HEADER_FLAGS_COLLECTOR) as u64 == 0;
    let mask_of = |from: usize, a0: ValueId| {
        insts[from..]
            .iter()
            .enumerate()
            .find_map(|(d, x)| match &x.kind {
                InstKind::BinOp(
                    op @ (BinOp::LShr | BinOp::And),
                    Operand::Value(a),
                    Operand::ConstI64(c),
                ) if *a == a0 => x.result.map(|r| (*op, *c as u64, r, from + d)),
                _ => None,
            })
    };
    for (i, load) in insts.iter().enumerate() {
        let (InstKind::Load(Type::I64, Operand::Value(p), 0), Some(t)) = (&load.kind, load.result)
        else {
            continue;
        };
        if !clean(p) || !single(t) {
            continue;
        }
        match mask_of(i + 1, t) {
            Some((BinOp::And, m, k, _)) if m & 0xffff_ffff_ffff == 0 && flags_only(m >> 48) => {
                chains.extend([t, k]);
            }
            Some((BinOp::LShr, 48, s, j)) if single(s) => {
                if let Some((BinOp::And, m, k, _)) = mask_of(j + 1, s)
                    && flags_only(m)
                {
                    chains.extend([t, s, k]);
                }
            }
            _ => {}
        }
    }
    chains
}

/// Operand occurrences per value, terminators included.
fn use_counts(func: &Function) -> HashMap<ValueId, u32> {
    let mut uses: HashMap<ValueId, u32> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| *uses.entry(v).or_insert(0) += 1);
        }
        match &block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => *uses.entry(*v).or_insert(0) += 1,
            _ => {}
        }
    }
    uses
}

/// `pre` jumps straight to `header` and is its only way in from
/// outside the loop. Otherwise a block between them (a diamond's arm,
/// `if (c) a.length = 0` just before the loop) could write the header
/// after the hoisted load has read it.
fn sole_entry(func: &Function, pre: BlockId, header: BlockId, body: &HashSet<BlockId>) -> bool {
    matches!(func.blocks[pre.0 as usize].term, Terminator::Br(h) if h == header)
        && func.blocks.iter().enumerate().all(|(bi, block)| {
            let b = BlockId(bi as u32);
            b == pre
                || body.contains(&b)
                || !match &block.term {
                    Terminator::Br(t) => *t == header,
                    Terminator::CondBr {
                        then_blk, else_blk, ..
                    } => *then_blk == header || *else_blk == header,
                    Terminator::Ret(_) | Terminator::Unreachable => false,
                }
        })
}

/// The header words some instruction in `body` may write, as
/// (array, offset), `None` standing for every word: a store through a
/// name of the array writes its own offset, a call handed a name of
/// the array or of its data pointer may write any.
fn written_headers(
    func: &Function,
    body: &HashSet<BlockId>,
    classes: &ArrayClasses,
    names: &[String],
) -> HashSet<(ValueId, Option<u64>)> {
    let mut dirty = HashSet::default();
    let mut named = |op: &Operand, with_base: bool, off: Option<u64>| {
        if let Operand::Value(v) = op {
            dirty.extend(classes.class.get(v).map(|a| (*a, off)));
            if with_base {
                dirty.extend(classes.base.get(v).map(|a| (*a, off)));
            }
        }
    };
    for &b in body {
        for inst in &func.blocks[b.0 as usize].insts {
            match &inst.kind {
                // a slot write through the data pointer stays past +40
                InstKind::Store(_, p, off) => named(p, false, Some(*off)),
                InstKind::StoreDyn(_, p, _) | InstKind::StoreDynScaled8(_, p, _) => {
                    named(p, false, None)
                }
                InstKind::Call(f, _)
                    if HEADER_READ_ONLY_CALLS
                        .contains(&names.get(f.0 as usize).map(String::as_str).unwrap_or("")) => {}
                InstKind::Call(_, args) => args.iter().for_each(|a| named(a, true, None)),
                // a release may free the cell
                InstKind::RcDecAny(v) => named(v, true, None),
                InstKind::CallIndirect(_, callee, args) => {
                    named(callee, true, None);
                    args.iter().for_each(|a| named(a, true, None));
                }
                _ => {}
            }
        }
    }
    dirty
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, FuncId, IPred, ValueInfo};

    fn decl(name: &str) -> Function {
        Function {
            name: name.into(),
            params: vec![],
            ret: Type::Ptr,
            blocks: vec![],
            values: vec![],
            current_origin: None,
        }
    }
    fn mk(result: Option<u32>, kind: InstKind) -> Inst {
        Inst {
            result: result.map(ValueId),
            kind,
            origin: None,
        }
    }
    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }

    /// b0: %0 = arr_alloc(8) · `entry` · Br b1 | b1 (header): %1 = len of
    /// %0 · %2 = 0 < %1 · CondBr b2 / b3 | b2 (body): %3 = data of %0 ·
    /// `body` · Br b1 | b3: Ret — `for (…; i < a.length; …) a[i]`.
    fn walk(entry_term: Terminator, body: Vec<Inst>) -> Module {
        let mut body_insts = vec![mk(Some(3), InstKind::Load(Type::Ptr, v(0), 32))];
        body_insts.extend(body);
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![mk(
                    Some(0),
                    InstKind::Call(FuncId(0), vec![Operand::ConstI64(8)]),
                )],
                term: entry_term,
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    mk(Some(1), InstKind::Load(Type::I64, v(0), 8)),
                    mk(
                        Some(2),
                        InstKind::ICmp(IPred::Slt, Operand::ConstI64(0), v(1)),
                    ),
                ],
                term: Terminator::CondBr {
                    cond: v(2),
                    then_blk: BlockId(2),
                    else_blk: BlockId(3),
                },
            },
            Block {
                id: BlockId(2),
                insts: body_insts,
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(3),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        // a diamond arm between the entry and the header
        if !matches!(blocks[0].term, Terminator::Br(_)) {
            blocks.push(Block {
                id: BlockId(4),
                insts: vec![],
                term: Terminator::Br(BlockId(1)),
            });
        }
        let ty = |t| ValueInfo { ty: t, name: None };
        Module {
            funcs: vec![
                decl("__torajs_arr_alloc"),
                decl("__torajs_arr_push"),
                decl("__torajs_arr_delete_tail_guard"),
                Function {
                    name: "main".into(),
                    params: vec![],
                    ret: Type::I64,
                    blocks,
                    values: vec![
                        ty(Type::Ptr),
                        ty(Type::I64),
                        ty(Type::Bool),
                        ty(Type::Ptr),
                        ty(Type::Ptr),
                    ],
                    current_origin: None,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_read_only_walk_reads_the_header_once() {
        let mut m = walk(Terminator::Br(BlockId(1)), vec![]);
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 2);
        let f = &m.funcs[3];
        assert_eq!(f.blocks[0].insts.len(), 3);
        assert_eq!(f.blocks[1].insts.len(), 1);
        assert!(f.blocks[2].insts.is_empty());
    }

    #[test]
    fn a_push_onto_the_walked_array_keeps_both_loads() {
        let push = mk(
            Some(4),
            InstKind::Call(FuncId(1), vec![v(0), Operand::ConstI64(1)]),
        );
        let mut m = walk(Terminator::Br(BlockId(1)), vec![push]);
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 0);
    }

    #[test]
    fn a_length_store_in_the_loop_keeps_only_the_length_load() {
        let store = mk(None, InstKind::Store(Operand::ConstI64(1), v(0), 8));
        let mut m = walk(Terminator::Br(BlockId(1)), vec![store]);
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 1);
        let f = &m.funcs[3];
        assert!(matches!(f.blocks[1].insts[0].kind, InstKind::Load(_, _, 8)));
        assert!(matches!(
            f.blocks[0].insts[1].kind,
            InstKind::Load(_, _, 32)
        ));
    }

    #[test]
    fn a_length_lock_test_leaves_the_loop_whole() {
        let lock = vec![
            mk(Some(5), InstKind::Load(Type::I64, v(0), 0)),
            mk(
                Some(6),
                InstKind::BinOp(BinOp::LShr, v(5), Operand::ConstI64(48)),
            ),
            mk(
                Some(7),
                InstKind::BinOp(BinOp::And, v(6), Operand::ConstI64(0x8290)),
            ),
            mk(
                Some(8),
                InstKind::ICmp(IPred::Ne, v(7), Operand::ConstI64(0)),
            ),
        ];
        let mut m = walk(Terminator::Br(BlockId(1)), lock.clone());
        for _ in 0..4 {
            m.funcs[3].values.push(ValueInfo {
                ty: Type::I64,
                name: None,
            });
        }
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 5);
        assert_eq!(m.funcs[3].blocks[2].insts.len(), 1);
        // a mask that reads the collector's buffered bit stays
        let mut buffered = lock;
        buffered[2].kind = InstKind::BinOp(BinOp::And, v(6), Operand::ConstI64(1 << 5));
        let mut m = walk(Terminator::Br(BlockId(1)), buffered);
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 2);
    }

    #[test]
    fn a_length_lock_masked_in_place_leaves_the_loop_whole() {
        let lock = |mask: i64| {
            vec![
                mk(Some(5), InstKind::Load(Type::I64, v(0), 0)),
                mk(
                    Some(6),
                    InstKind::BinOp(BinOp::And, v(5), Operand::ConstI64(mask)),
                ),
                mk(
                    Some(7),
                    InstKind::ICmp(IPred::Ne, v(6), Operand::ConstI64(0)),
                ),
            ]
        };
        let grow = |m: &mut Module| {
            for _ in 0..3 {
                m.funcs[3].values.push(ValueInfo {
                    ty: Type::I64,
                    name: None,
                });
            }
        };
        let mut m = walk(Terminator::Br(BlockId(1)), lock((0x8290u64 << 48) as i64));
        grow(&mut m);
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 4);
        assert_eq!(m.funcs[3].blocks[2].insts.len(), 1);
        // the collector's buffered bit, or any bit below the flags, stays
        for mask in [1i64 << 53, (0x8290 << 48) | 1] {
            let mut m = walk(Terminator::Br(BlockId(1)), lock(mask));
            grow(&mut m);
            assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 2);
        }
    }

    #[test]
    fn the_pop_guard_does_not_write_the_header() {
        let guard = mk(None, InstKind::Call(FuncId(2), vec![v(0)]));
        let mut m = walk(Terminator::Br(BlockId(1)), vec![guard]);
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 2);
    }

    #[test]
    fn a_slot_write_through_the_data_pointer_does_not_stop_it() {
        let slot = mk(None, InstKind::Store(Operand::ConstI64(1), v(3), 0));
        let mut m = walk(Terminator::Br(BlockId(1)), vec![slot]);
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 2);
    }

    #[test]
    fn a_second_way_into_the_header_keeps_both_loads() {
        let diamond = Terminator::CondBr {
            cond: Operand::ConstBool(true),
            then_blk: BlockId(1),
            else_blk: BlockId(4),
        };
        let mut m = walk(diamond, vec![]);
        assert_eq!(hoist_header_loads(&mut m, None).loads_hoisted, 0);
    }
}
