//! Header words in registers across a loop whose only writers of them
//! are the typed element store's cold-arm kernels.
//!
//! `stack[sp] = v` in a loop reads the array's length, its flags word
//! and its data pointer on every store, because the out-of-range arm
//! calls `__torajs_arr_typed_set_grow` and the guarded arm calls the
//! refusal and revive kernels: each call names the array, so
//! `load_licm` leaves every header load of it in place. The calls sit
//! on arms the loop almost never takes.
//!
//! This is the same scalar promotion `len_promote` does (LLVM LICM's
//! `promoteLoopAccessesToScalars`; Lu & Cooper, PLDI 97) for words the
//! loop only reads, on the same allocation-site alias model: for a
//! live tracked array A, only instructions naming A can touch its
//! header. Inside the loop each read word lives in a register cell
//! `c` (a multi-def Copy value, the IR's φ form):
//!
//! - the preheader reads it once: `c = copy (load A + off)`;
//! - a load of it becomes `r = copy c`, or hands its uses `c` itself
//!   when no re-read lies between it and them;
//! - right after each header kernel handed A, the word is read again
//!   into `c`, so whatever the kernel wrote is what the next read sees.
//!
//! Memory is never written here, so no write-back is needed. A loop is
//! taken only if it calls one of the kernels on A, every other call
//! naming A only reads its header (`HEADER_READ_ONLY_CALLS`), nothing
//! in it releases A or stores through A at a promoted offset. The flags
//! word (+0) is cached only as its masked tests, one cell per mask, and
//! only when every read of it in the loop is masked to flag bits the
//! cycle collector never flips: those move only inside a call handed
//! A, while the refcount and collector bits may not, and the mask
//! throws them away. A loop whose header has more than one way in from
//! outside gets a preheader block of its own.
//!
//! `TORAJS_LOAD_REREAD_OFF=1` skips, `TORAJS_LOAD_REREAD_STATS=1` counts.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId, ValueInfo,
};

use crate::interval::elem::{ArrayClasses, array_classes};
use crate::slot_forward::{resolve, rewrite_operands};
use plan::{ARR_WORD_OFF, Word, flag_masks, header_kernel, plan, word_load};
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

mod cfg;
mod plan;
#[cfg(test)]
mod tests;

#[derive(Debug, Default)]
pub struct LoadRereadStats {
    /// (loop, array, word) cells promoted.
    pub cells_promoted: u32,
    /// Re-reads placed after header kernels.
    pub rereads_placed: u32,
    /// Preheader blocks made for loops entered from several blocks.
    pub preheaders_made: u32,
    /// Reads whose uses now name the cell itself.
    pub reads_direct: u32,
}

pub fn reread_header_words(module: &mut Module, known: Option<&[String]>) -> LoadRereadStats {
    let names = crate::interval::module_names(known, module);
    let mut stats = LoadRereadStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        if let Some(classes) = array_classes(func, &names)
            && !classes.live.is_empty()
        {
            promote_fn(func, &classes, &names, &mut stats);
        }
    }
    stats
}

fn promote_fn(
    func: &mut Function,
    classes: &ArrayClasses,
    names: &[String],
    stats: &mut LoadRereadStats,
) {
    let dom = DominatorTree::compute(func);
    let mut loops = LoopAnalysis::compute(func, &dom).loops().to_vec();
    // outer first; a loop inside one promoted for A was rewritten with it
    loops.sort_by_key(|l| std::cmp::Reverse(l.body.len()));
    let mut def_block: HashMap<ValueId, BlockId> = HashMap::default();
    for (bi, block) in func.blocks.iter().enumerate() {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                def_block.insert(r, BlockId(bi as u32));
            }
        }
    }
    let masks = flag_masks(func);
    let mut roots: Vec<ValueId> = classes.live.iter().copied().collect();
    roots.sort_by_key(|v| v.0);
    let mut done: HashMap<ValueId, HashSet<BlockId>> = HashMap::default();
    for lp in &loops {
        if dom.immediate_dominator(lp.header).is_none() {
            continue;
        }
        let body: HashSet<BlockId> = lp.body.iter().copied().collect();
        let mut blocks = lp.body.clone();
        blocks.sort_by_key(|b| b.0);
        for &a in &roots {
            if done.get(&a).is_some_and(|d| d.contains(&lp.header)) {
                continue;
            }
            let Some(&root_blk) = def_block.get(&a) else {
                continue;
            };
            if body.contains(&root_blk) || !dom.dominates(root_blk, lp.header) {
                continue;
            }
            let Some(cells) = plan(func, &blocks, &body, a, classes, names, &masks) else {
                continue;
            };
            let site = Site {
                blocks: &blocks,
                header: lp.header,
                pre: cfg::preheader(func, lp.header, &body, stats),
            };
            promote(func, &site, a, &cells, classes, names, stats);
            done.entry(a).or_default().extend(blocks.iter().copied());
        }
    }
}

fn inst(result: Option<ValueId>, kind: InstKind) -> Inst {
    Inst {
        result,
        kind,
        origin: None,
    }
}

/// One promoted loop: the blocks of its body, its header and the
/// preheader the first reads go to.
struct Site<'b> {
    blocks: &'b [BlockId],
    header: BlockId,
    pre: BlockId,
}

fn promote(
    func: &mut Function,
    site: &Site,
    a: ValueId,
    words: &[Word],
    classes: &ArrayClasses,
    names: &[String],
    stats: &mut LoadRereadStats,
) {
    let fresh = |func: &mut Function, ty: Type| {
        let v = ValueId(func.values.len() as u32);
        func.values.push(ValueInfo { ty, name: None });
        v
    };
    let root = Operand::Value(a);
    let cells: Vec<(Word, ValueId)> = words.iter().map(|&w| (w, fresh(func, w.ty))).collect();
    // flags-word loads: each use is one of the masked tests that become
    // cells, so the load itself goes
    let flag_loads: HashSet<ValueId> = site
        .blocks
        .iter()
        .flat_map(|b| &func.blocks[b.0 as usize].insts)
        .filter(|i| word_load(&i.kind, a, classes).is_some_and(|(off, _)| off == ARR_WORD_OFF))
        .filter_map(|i| i.result)
        .collect();
    let flags_cached = cells.iter().any(|(w, _)| w.mask.is_some());
    let cell_of = |kind: &InstKind| {
        let w = match kind {
            InstKind::BinOp(BinOp::And, Operand::Value(r), Operand::ConstI64(k))
                if flags_cached && flag_loads.contains(r) =>
            {
                Word {
                    off: ARR_WORD_OFF,
                    ty: Type::I64,
                    mask: Some(*k),
                }
            }
            k => {
                let (off, ty) = word_load(k, a, classes).filter(|(o, _)| *o != ARR_WORD_OFF)?;
                Word {
                    off,
                    ty,
                    mask: None,
                }
            }
        };
        cells.iter().find(|(x, _)| *x == w).map(|(x, c)| (x.ty, *c))
    };
    let mut reads: HashMap<ValueId, (BlockId, usize)> = HashMap::default();
    let mut rereads: HashMap<BlockId, Vec<usize>> = HashMap::default();
    for &b in site.blocks {
        for (i, x) in func.blocks[b.0 as usize].insts.iter().enumerate() {
            if let (Some(r), Some(_)) = (x.result, cell_of(&x.kind)) {
                reads.insert(r, (b, i));
            } else if header_kernel(&x.kind, a, classes, names) {
                rereads.entry(b).or_default().push(i);
            }
        }
    }
    let direct = cfg::direct_reads(func, site.header, &reads, &rereads);
    let read_into = |func: &mut Function, out: &mut Vec<Inst>| {
        let mut word: Option<ValueId> = None;
        for &(w, c) in &cells {
            let t = match w.mask {
                None => {
                    let t = fresh(func, w.ty);
                    out.push(inst(Some(t), InstKind::Load(w.ty, root, w.off)));
                    t
                }
                Some(k) => {
                    let t = *word.get_or_insert_with(|| {
                        let t = fresh(func, Type::I64);
                        out.push(inst(Some(t), InstKind::Load(Type::I64, root, w.off)));
                        t
                    });
                    let m = fresh(func, Type::I64);
                    let and = InstKind::BinOp(BinOp::And, Operand::Value(t), Operand::ConstI64(k));
                    out.push(inst(Some(m), and));
                    m
                }
            };
            out.push(inst(Some(c), InstKind::Copy(w.ty, Operand::Value(t))));
        }
    };
    let mut head = Vec::new();
    read_into(func, &mut head);
    func.blocks[site.pre.0 as usize].insts.extend(head);
    let mut replace: HashMap<ValueId, Operand> = HashMap::default();
    for &b in site.blocks {
        let old = std::mem::take(&mut func.blocks[b.0 as usize].insts);
        let mut out = Vec::with_capacity(old.len());
        for i in old {
            if flags_cached && i.result.is_some_and(|r| flag_loads.contains(&r)) {
                continue;
            }
            if let Some((ty, c)) = cell_of(&i.kind) {
                match i.result {
                    Some(r) if direct.contains(&r) => {
                        replace.insert(r, Operand::Value(c));
                    }
                    _ => out.push(Inst {
                        kind: InstKind::Copy(ty, Operand::Value(c)),
                        ..i
                    }),
                }
                continue;
            }
            let kernel = header_kernel(&i.kind, a, classes, names);
            out.push(i);
            if kernel {
                read_into(func, &mut out);
                stats.rereads_placed += 1;
            }
        }
        func.blocks[b.0 as usize].insts = out;
    }
    if !replace.is_empty() {
        for block in func.blocks.iter_mut() {
            for inst in block.insts.iter_mut() {
                rewrite_operands(&mut inst.kind, &replace);
            }
            match &mut block.term {
                Terminator::Ret(Some(op)) => *op = resolve(op, &replace),
                Terminator::CondBr { cond, .. } => *cond = resolve(cond, &replace),
                _ => {}
            }
        }
    }
    stats.cells_promoted += cells.len() as u32;
    stats.reads_direct += replace.len() as u32;
}
