//! Array length promotion — `while (xs.length > 0) total += xs.pop()`
//! keeps the length in memory: the loop reads `xs + 8` at the top,
//! the pop writes it back, and the next iteration's read waits on that
//! store. `load_licm` cannot help, because the loop writes the word.
//!
//! This is scalar promotion of a loop-invariant address (LLVM LICM's
//! `promoteLoopAccessesToScalars`; Lu & Cooper, "Register Promotion in
//! C Programs", PLDI 97) on the allocation-site alias model
//! `interval::elem` keeps: for a live tracked array A, every access to
//! its length word is a `Load` / `Store` at +8 through a name of A, or
//! a call handed a name of A. So inside one loop the word can live in
//! a register cell `c`:
//!
//! - the preheader reads it once: `c = copy (load A +8)`;
//! - a load of it becomes `r = copy c`, a store of `v` becomes
//!   `c = copy v` (`c` is a multi-def Copy value, the IR's φ form);
//! - before any instruction that may run code outside the function
//!   (a call, a release, a box), `store c, A +8` puts the word back,
//!   so the runtime and the cycle collector see the true length;
//! - a call naming A on the re-read list gets the write-back and a
//!   re-read after it: the length-lock raisers (they may shift
//!   elements), and the push / shift kernels a guard's slow arm calls
//!   (they move the length, never free the array and keep no copy of
//!   the length elsewhere); any other call naming A (reserve, a drop,
//!   a user function, a push codegen opens) leaves the loop alone;
//! - every edge out of the loop stores `c` back.
//!
//! A loop is taken only if it stores the length at least once; a loop
//! that just reads it is `load_licm`'s. Loops are tried outermost
//! first, and a loop inside one already promoted for A is skipped —
//! its accesses were rewritten with the outer one, and rewriting the
//! outer loop's write-backs again would turn them into copies.
//!
//! `TORAJS_LEN_PROMOTE_OFF=1` skips, `TORAJS_LEN_PROMOTE_STATS=1` counts.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    Block, BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId, ValueInfo,
};

use crate::interval::elem::{ArrayClasses, array_classes};
use crate::optimize::classify_pure;
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::{LoopAnalysis, NaturalLoop};

#[derive(Debug, Default)]
pub struct LenPromoteStats {
    /// (loop, array) pairs whose length word moved into a register.
    pub cells_promoted: u32,
    /// Write-backs placed before calls and on exit edges.
    pub stores_placed: u32,
}

const ARR_LEN_OFF: u64 = 8;

/// Calls naming A the cell lives across, written back before and
/// re-read after. The raisers read the array's flags and either return
/// or record a TypeError; the kernels are the out-of-line arms of an
/// opened push and of a shift on a marked array. Each may write the
/// length and none frees the array or stores its length anywhere else.
/// `__torajs_arr_push` stays off: codegen opens it in place, and the
/// re-read around every such push costs more than the cell saves.
const REREAD_CALLS: &[&str] = &[
    "__torajs_arr_delete_tail_guard",
    "__torajs_arr_shift_guard",
    "__torajs_arr_push_cold",
    "__torajs_arr_shift",
];

pub fn promote_lengths(module: &mut Module, known: Option<&[String]>) -> LenPromoteStats {
    let names = crate::interval::module_names(known, module);
    let mut stats = LenPromoteStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        if let Some(classes) = array_classes(func, &names) {
            promote_fn(func, &classes, &names, &mut stats);
        }
    }
    stats
}

fn promote_fn(
    func: &mut Function,
    classes: &ArrayClasses,
    names: &[String],
    stats: &mut LenPromoteStats,
) {
    if classes.live.is_empty() {
        return;
    }
    let dom = DominatorTree::compute(func);
    let mut loops = LoopAnalysis::compute(func, &dom).loops().to_vec();
    loops.sort_by_key(|l| std::cmp::Reverse(l.body.len()));
    let mut def_block: HashMap<ValueId, BlockId> = HashMap::default();
    for (bi, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                def_block.insert(r, BlockId(bi as u32));
            }
        }
    }
    let mut roots: Vec<ValueId> = classes.live.iter().copied().collect();
    roots.sort_by_key(|v| v.0);
    // blocks already covered by a promoted loop, per array
    let mut done: HashMap<ValueId, HashSet<BlockId>> = HashMap::default();
    for lp in &loops {
        let Some(pre) = dom.immediate_dominator(lp.header) else {
            continue;
        };
        let body: HashSet<BlockId> = lp.body.iter().copied().collect();
        if pre == lp.header || body.contains(&pre) || !sole_entry(func, pre, lp.header, &body) {
            continue;
        }
        for &a in &roots {
            if done.get(&a).is_some_and(|d| d.contains(&lp.header)) {
                continue;
            }
            let Some(&root_blk) = def_block.get(&a) else {
                continue;
            };
            if !dom.dominates(root_blk, pre) || !qualifies(func, lp, a, classes, names) {
                continue;
            }
            promote_loop(func, lp, pre, a, classes, names, stats);
            done.entry(a).or_default().extend(lp.body.iter().copied());
        }
    }
}

/// Same shape `load_licm` asks of its preheader: `pre` jumps straight
/// to `header` and nothing else outside the loop does.
fn sole_entry(func: &Function, pre: BlockId, header: BlockId, body: &HashSet<BlockId>) -> bool {
    matches!(func.blocks[pre.0 as usize].term, Terminator::Br(h) if h == header)
        && func.blocks.iter().enumerate().all(|(bi, block)| {
            let b = BlockId(bi as u32);
            b == pre || body.contains(&b) || !succs(&block.term).contains(&header)
        })
}

fn succs(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Br(t) => vec![*t],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}

fn names_array(op: &Operand, a: ValueId, classes: &ArrayClasses) -> bool {
    match op {
        Operand::Value(v) => classes.class.get(v) == Some(&a) || classes.base.get(v) == Some(&a),
        _ => false,
    }
}

fn is_len_access(kind: &InstKind, a: ValueId, classes: &ArrayClasses) -> bool {
    match kind {
        InstKind::Load(_, Operand::Value(p), ARR_LEN_OFF)
        | InstKind::Store(_, Operand::Value(p), ARR_LEN_OFF) => classes.class.get(p) == Some(&a),
        _ => false,
    }
}

fn reread_call(kind: &InstKind, names: &[String]) -> bool {
    matches!(kind, InstKind::Call(f, _)
        if REREAD_CALLS.contains(&names.get(f.0 as usize).map(String::as_str).unwrap_or("")))
}

/// The loop stores A's length, every length access is an i64 word, and
/// nothing in it that names A can write the length behind the cell's
/// back or free the array.
fn qualifies(
    func: &Function,
    lp: &NaturalLoop,
    a: ValueId,
    classes: &ArrayClasses,
    names: &[String],
) -> bool {
    let mut stores = false;
    for &b in &lp.body {
        for inst in &func.blocks[b.0 as usize].insts {
            match &inst.kind {
                k if is_len_access(k, a, classes) => match k {
                    InstKind::Load(ty, ..) if *ty != Type::I64 => return false,
                    InstKind::Store(..) => stores = true,
                    _ => {}
                },
                k @ InstKind::Call(_, args) if args.iter().any(|x| names_array(x, a, classes)) => {
                    if !reread_call(k, names) {
                        return false;
                    }
                }
                InstKind::CallIndirect(_, callee, args)
                    if names_array(callee, a, classes)
                        || args.iter().any(|x| names_array(x, a, classes)) =>
                {
                    return false;
                }
                InstKind::RcDecAny(v) if names_array(v, a, classes) => return false,
                _ => {}
            }
        }
    }
    stores
}

/// Instructions that stay inside the function: nothing they do can
/// reach A's length word except through the accesses rewritten here.
fn stays_local(kind: &InstKind) -> bool {
    classify_pure(kind)
        || matches!(
            kind,
            InstKind::Load(..)
                | InstKind::LoadDyn(..)
                | InstKind::LoadDynScaled8(..)
                | InstKind::Store(..)
                | InstKind::StoreDyn(..)
                | InstKind::StoreDynScaled8(..)
                | InstKind::Copy(..)
                | InstKind::Identity(..)
                // register bit moves around a slot's raw bytes
                | InstKind::BitCastF64ToI64(..)
                | InstKind::BitCastI64ToF64(..)
        )
}

fn inst(result: Option<ValueId>, kind: InstKind) -> Inst {
    Inst {
        result,
        kind,
        origin: None,
    }
}

fn promote_loop(
    func: &mut Function,
    lp: &NaturalLoop,
    pre: BlockId,
    a: ValueId,
    classes: &ArrayClasses,
    names: &[String],
    stats: &mut LenPromoteStats,
) {
    let fresh = |func: &mut Function| {
        let v = ValueId(func.values.len() as u32);
        func.values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        v
    };
    let cell = fresh(func);
    let root = Operand::Value(a);
    let write_back = || {
        inst(
            None,
            InstKind::Store(Operand::Value(cell), root, ARR_LEN_OFF),
        )
    };

    let t = fresh(func);
    let pre_insts = &mut func.blocks[pre.0 as usize].insts;
    pre_insts.push(inst(Some(t), InstKind::Load(Type::I64, root, ARR_LEN_OFF)));
    pre_insts.push(inst(
        Some(cell),
        InstKind::Copy(Type::I64, Operand::Value(t)),
    ));

    let body: HashSet<BlockId> = lp.body.iter().copied().collect();
    let mut blocks: Vec<BlockId> = lp.body.clone();
    blocks.sort_by_key(|b| b.0);
    for b in blocks {
        let old = std::mem::take(&mut func.blocks[b.0 as usize].insts);
        let mut out = Vec::with_capacity(old.len() + 2);
        // memory holds the cell's value until the cell is redefined
        let mut synced = false;
        for i in old {
            if is_len_access(&i.kind, a, classes) {
                match i.kind {
                    InstKind::Load(..) => out.push(Inst {
                        kind: InstKind::Copy(Type::I64, Operand::Value(cell)),
                        ..i
                    }),
                    InstKind::Store(v, ..) => {
                        out.push(Inst {
                            result: Some(cell),
                            kind: InstKind::Copy(Type::I64, v),
                            origin: i.origin,
                        });
                        synced = false;
                    }
                    _ => unreachable!(),
                }
                continue;
            }
            if stays_local(&i.kind) {
                out.push(i);
                continue;
            }
            if !synced {
                out.push(write_back());
                stats.stores_placed += 1;
                synced = true;
            }
            let reread = reread_call(&i.kind, names)
                && matches!(&i.kind, InstKind::Call(_, args)
                    if args.iter().any(|x| names_array(x, a, classes)));
            out.push(i);
            if reread {
                let r = fresh(func);
                out.push(inst(Some(r), InstKind::Load(Type::I64, root, ARR_LEN_OFF)));
                out.push(inst(
                    Some(cell),
                    InstKind::Copy(Type::I64, Operand::Value(r)),
                ));
            }
        }
        func.blocks[b.0 as usize].insts = out;
    }

    // every edge leaving the loop puts the word back
    let mut preds: HashMap<BlockId, Vec<BlockId>> = HashMap::default();
    for (bi, block) in func.blocks.iter().enumerate() {
        for s in succs(&block.term) {
            preds.entry(s).or_default().push(BlockId(bi as u32));
        }
    }
    let mut exits: Vec<(BlockId, BlockId)> = Vec::new();
    for &b in &lp.body {
        for s in succs(&func.blocks[b.0 as usize].term) {
            if !body.contains(&s) && !exits.contains(&(b, s)) {
                exits.push((b, s));
            }
        }
    }
    exits.sort_by_key(|(b, s)| (b.0, s.0));
    let mut headed: HashSet<BlockId> = HashSet::default();
    for (b, s) in exits {
        if preds[&s].iter().all(|p| body.contains(p)) {
            if headed.insert(s) {
                func.blocks[s.0 as usize].insts.insert(0, write_back());
                stats.stores_placed += 1;
            }
            continue;
        }
        let split = BlockId(func.blocks.len() as u32);
        func.blocks.push(Block {
            id: split,
            insts: vec![write_back()],
            term: Terminator::Br(s),
        });
        stats.stores_placed += 1;
        if let Terminator::CondBr {
            then_blk, else_blk, ..
        } = &mut func.blocks[b.0 as usize].term
        {
            if *then_blk == s {
                *then_blk = split;
            }
            if *else_blk == s {
                *else_blk = split;
            }
        }
    }
    stats.cells_promoted += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, FuncId, IPred};

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
    fn v(i: u32) -> Operand {
        Operand::Value(ValueId(i))
    }

    /// b0: %0 = arr_alloc(8) · Br b1 | b1 (header): %1 = len of %0 ·
    /// %2 = %1 > 0 · CondBr b2 / b3 | b2 (body): `pre` · %3 = %1 - 1 ·
    /// store %3 → len · `post` · Br b1 | b3: Ret — the pop loop.
    fn pop_loop(pre: Vec<Inst>, post: Vec<Inst>) -> Module {
        let mut body = pre;
        body.push(inst(
            Some(ValueId(3)),
            InstKind::BinOp(BinOp::Sub, v(1), Operand::ConstI64(1)),
        ));
        body.push(inst(None, InstKind::Store(v(3), v(0), ARR_LEN_OFF)));
        body.extend(post);
        let blocks = vec![
            Block {
                id: BlockId(0),
                insts: vec![inst(
                    Some(ValueId(0)),
                    InstKind::Call(FuncId(0), vec![Operand::ConstI64(8)]),
                )],
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![
                    inst(
                        Some(ValueId(1)),
                        InstKind::Load(Type::I64, v(0), ARR_LEN_OFF),
                    ),
                    inst(
                        Some(ValueId(2)),
                        InstKind::ICmp(IPred::Sgt, v(1), Operand::ConstI64(0)),
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
                insts: body,
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(3),
                insts: vec![],
                term: Terminator::Ret(None),
            },
        ];
        let ty = |t| ValueInfo { ty: t, name: None };
        Module {
            funcs: vec![
                decl("__torajs_arr_alloc"),
                decl("__torajs_arr_push"),
                decl("__torajs_arr_delete_tail_guard"),
                decl("print_f64"),
                Function {
                    name: "main".into(),
                    params: vec![],
                    ret: Type::I64,
                    blocks,
                    values: vec![
                        ty(Type::Ptr),
                        ty(Type::I64),
                        ty(Type::Bool),
                        ty(Type::I64),
                        ty(Type::Ptr),
                    ],
                    current_origin: None,
                },
                decl("__torajs_arr_push_cold"),
            ],
            ..Default::default()
        }
    }

    fn is_len_store(i: &Inst) -> bool {
        matches!(i.kind, InstKind::Store(_, _, ARR_LEN_OFF))
    }
    fn is_len_load(i: &Inst) -> bool {
        matches!(i.kind, InstKind::Load(_, _, ARR_LEN_OFF))
    }

    #[test]
    fn the_pop_loop_keeps_its_length_in_a_register() {
        let mut m = pop_loop(vec![], vec![]);
        let st = promote_lengths(&mut m, None);
        assert_eq!((st.cells_promoted, st.stores_placed), (1, 1));
        let f = &m.funcs[4];
        // the preheader reads the word once into the cell
        assert!(is_len_load(&f.blocks[0].insts[1]));
        let cell = f.blocks[0].insts[2].result.unwrap();
        assert!(matches!(f.blocks[0].insts[2].kind, InstKind::Copy(..)));
        // neither the header nor the body touches memory for it
        let loop_insts = f.blocks[1].insts.iter().chain(&f.blocks[2].insts);
        assert!(
            loop_insts
                .clone()
                .all(|i| !is_len_load(i) && !is_len_store(i))
        );
        assert!(matches!(&f.blocks[1].insts[0].kind,
            InstKind::Copy(_, Operand::Value(c)) if *c == cell));
        assert_eq!(f.blocks[2].insts.last().unwrap().result, Some(cell));
        // the exit puts it back
        assert!(is_len_store(&f.blocks[3].insts[0]));
    }

    #[test]
    fn a_push_in_the_loop_leaves_it_in_memory() {
        let push = inst(
            Some(ValueId(4)),
            InstKind::Call(FuncId(1), vec![v(0), Operand::ConstI64(1)]),
        );
        let mut m = pop_loop(vec![push], vec![]);
        assert_eq!(promote_lengths(&mut m, None).cells_promoted, 0);
    }

    #[test]
    fn a_read_only_loop_is_left_to_load_licm() {
        let mut m = pop_loop(vec![], vec![]);
        m.funcs[4].blocks[2].insts.retain(|i| !is_len_store(i));
        assert_eq!(promote_lengths(&mut m, None).cells_promoted, 0);
    }

    #[test]
    fn a_guard_call_sees_the_word_and_is_followed_by_a_reread() {
        let guard = inst(None, InstKind::Call(FuncId(2), vec![v(0)]));
        let mut m = pop_loop(vec![guard], vec![]);
        assert_eq!(promote_lengths(&mut m, None).cells_promoted, 1);
        let b = &m.funcs[4].blocks[2].insts;
        assert!(is_len_store(&b[0]));
        assert!(matches!(b[1].kind, InstKind::Call(FuncId(2), _)));
        assert!(is_len_load(&b[2]));
        assert!(matches!(b[3].kind, InstKind::Copy(..)));
    }

    #[test]
    fn an_opened_pushs_cold_arm_sees_the_word_and_is_followed_by_a_reread() {
        let bits = inst(
            Some(ValueId(4)),
            InstKind::BitCastF64ToI64(Operand::ConstF64(1.0)),
        );
        let push = inst(
            Some(ValueId(5)),
            InstKind::Call(FuncId(5), vec![v(0), v(4)]),
        );
        let mut m = pop_loop(vec![bits, push], vec![]);
        let vals = &mut m.funcs[4].values;
        vals[4].ty = Type::I64;
        vals.push(ValueInfo {
            ty: Type::Ptr,
            name: None,
        });
        assert_eq!(promote_lengths(&mut m, None).cells_promoted, 1);
        let b = &m.funcs[4].blocks[2].insts;
        // the bit move stays ahead of the write-back
        assert!(matches!(b[0].kind, InstKind::BitCastF64ToI64(_)));
        assert!(is_len_store(&b[1]));
        assert!(matches!(b[2].kind, InstKind::Call(FuncId(5), _)));
        assert!(is_len_load(&b[3]));
        assert!(matches!(b[4].kind, InstKind::Copy(..)));
    }

    #[test]
    fn an_unrelated_call_after_the_store_sees_the_new_length() {
        let print = inst(
            None,
            InstKind::Call(FuncId(3), vec![Operand::ConstF64(1.0)]),
        );
        let mut m = pop_loop(vec![], vec![print]);
        assert_eq!(promote_lengths(&mut m, None).cells_promoted, 1);
        let b = &m.funcs[4].blocks[2].insts;
        let n = b.len();
        assert!(is_len_store(&b[n - 2]));
        assert!(matches!(b[n - 1].kind, InstKind::Call(FuncId(3), _)));
        assert!(!is_len_load(&b[n - 1]));
    }
}
