//! Filling the `__drop_<C>` bodies (`class_drop`) once every layout is
//! final, and the per-cell tests in them that a whole-program proof
//! may settle later ([`DropHooks`]).
//!
//! A release fn tests every cell it reaches against what a slot of the
//! sid could hold: the undefined sentinel, a cell of another layout
//! (the tag test), a cell with an expando bag, a cell a WeakRef
//! observes, a cell the cycle buffer holds; and it buffers a cell whose
//! count stays above zero. Which of those a
//! slot can hold is a question about the whole finished module, asked
//! after lowering (`ssa_lower_pass_2_5::drop_proofs`). Each test is
//! emitted as always and recorded with the block whose branch it
//! feeds; a proven test fixes that branch to the way it then always
//! goes, and the optimizer removes the test with the dead leg.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ast::PropKey;
use crate::ssa::{self, BlockId, FuncId, Module, Operand, Terminator, Type};
use crate::ssa_lower::Intrinsics;
use crate::ssa_lower_emit_drop_value::class_drop::{Cx, branch, emit_release};
use crate::ssa_lower_emit_drop_value::class_drop_cells::emit_is_undef;
use crate::ssa_lower_emit_drop_value::class_drop_setup::ClassDropFn;
use crate::ssa_lower_emit_drop_value::class_drop_worklist as worklist;

/// A per-cell test of a release fn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Test {
    /// A worklist's cell is the undefined sentinel. Settled, it holds
    /// for every cell taken from an own-sid field, and the root is
    /// tested once in the prelude instead ([`Test::UndefRoot`]).
    Undef,
    /// Where the worklist's root is tested once (the prelude), when the
    /// per-cell [`Test::Undef`] is settled.
    UndefRoot,
    /// The class tag is one of the sid's: a cell of the sid's layout.
    Tag,
    /// Something observes some cell through a WeakRef.
    WeakRef,
    /// The cell carries an expando bag.
    Props,
    /// The cycle buffer holds the dying cell.
    Buffered,
    /// The block whose branch sends a cell with references left to the
    /// cycle buffer; proven, that edge goes to the given block instead.
    BufferLeg(BlockId),
}

impl Test {
    /// The answer a proven test always gives.
    fn settled(self) -> bool {
        matches!(self, Test::Tag)
    }
}

/// One test: the fn, the sid of the cell tested, and the block whose
/// conditional branch the test feeds.
pub(crate) struct Hook {
    pub fid: FuncId,
    pub sid: u32,
    pub test: Test,
    pub blk: BlockId,
}

/// Every test the release fns made, with what the proofs read about
/// the sids: each sid's tag set, and which sids are class instances.
pub(crate) struct DropHooks {
    pub hooks: Vec<Hook>,
    pub tags: HashMap<u32, Vec<u32>>,
    pub class_sids: HashSet<u32>,
}

impl DropHooks {
    /// Fix the branch of every test `proven` answers for; answers how
    /// many were fixed.
    pub(crate) fn settle(&self, module: &mut Module, proven: impl Fn(u32, Test) -> bool) -> usize {
        let mut n = 0;
        let mut fids = Vec::new();
        for h in self.hooks.iter().filter(|h| proven(h.sid, h.test)) {
            let f = &mut module.funcs[h.fid.0 as usize];
            if h.test == Test::UndefRoot {
                test_root(f, h.blk);
                n += 1;
                continue;
            }
            let Terminator::CondBr { cond, else_blk, .. } = &mut f.blocks[h.blk.0 as usize].term
            else {
                continue;
            };
            match h.test {
                Test::BufferLeg(skip) => *else_blk = skip,
                t => *cond = Operand::ConstBool(t.settled()),
            }
            n += 1;
            fids.push(h.fid);
        }
        fids.dedup();
        for fid in fids {
            sweep(&mut module.funcs[fid.0 as usize]);
        }
        n
    }
}

/// Leave the release fn from `prelude` when its root is the undefined
/// sentinel.
fn test_root(f: &mut ssa::Function, prelude: BlockId) {
    let Terminator::Br(entry) = f.blocks[prelude.0 as usize].term else {
        unreachable!("a release fn's prelude branches to its entry");
    };
    let root = Operand::Value(f.params[0]);
    let done = f.add_block();
    f.set_term(done, Terminator::Ret(None));
    let is_undef = emit_is_undef(f, prelude, root);
    branch(f, prelude, is_undef, done, entry);
}

/// Drop what a fixed branch left without a use: the blocks no longer
/// reached, the test's compare and the loads, masks and addresses that
/// fed it. A release fn is not optimized further, so nothing else
/// would.
fn sweep(f: &mut ssa::Function) {
    let mut reached = vec![false; f.blocks.len()];
    let mut work = vec![0usize];
    while let Some(b) = work.pop() {
        if std::mem::replace(&mut reached[b], true) {
            continue;
        }
        match &f.blocks[b].term {
            Terminator::Br(t) => work.push(t.0 as usize),
            Terminator::CondBr {
                cond: Operand::ConstBool(c),
                then_blk,
                else_blk,
            } => work.push(if *c { then_blk.0 } else { else_blk.0 } as usize),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => work.extend([then_blk.0 as usize, else_blk.0 as usize]),
            _ => {}
        }
    }
    for (b, r) in f.blocks.iter_mut().zip(reached) {
        if !r {
            b.insts.clear();
            b.term = Terminator::Unreachable;
        }
    }
    loop {
        let mut used = vec![false; f.values.len()];
        for b in &f.blocks {
            for i in &b.insts {
                ssa::visit_value_operands(&i.kind, |v| used[v.0 as usize] = true);
            }
            if let Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) = &b.term
            {
                used[v.0 as usize] = true;
            }
        }
        let mut removed = false;
        for b in &mut f.blocks {
            b.insts.retain(|i| {
                let pure = matches!(
                    i.kind,
                    ssa::InstKind::ICmp(..)
                        | ssa::InstKind::Load(..)
                        | ssa::InstKind::GlobalRef(_)
                        | ssa::InstKind::BinOp(..)
                );
                let dead = pure && i.result.is_some_and(|r| !used[r.0 as usize]);
                removed |= dead;
                !dead
            });
        }
        if !removed {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn populate_class_drop_bodies(
    class_drops: &[ClassDropFn],
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    struct_layouts: &[Vec<(PropKey, Type)>],
    arr_layouts: &crate::ssa::ArrLayouts,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    intrinsics: &Intrinsics,
    module: &mut Module,
) -> DropHooks {
    let sid_of = |cn: &String| match aliases.get(cn) {
        Some(Type::Obj(s)) => Some(s.0),
        _ => None,
    };
    let mut class_sids: HashSet<u32> = class_name_to_tag.keys().filter_map(sid_of).collect();
    // sids a factory stamped a class tag onto besides the class's own
    for sid in 0..struct_layouts.len() as u32 {
        if anon_stamp_pool
            .borrow()
            .is_class_minted(crate::ssa::StructId(sid))
        {
            class_sids.insert(sid);
        }
    }
    let drop_fids: HashMap<u32, FuncId> = class_drops.iter().map(|d| (d.sid.0, d.fid)).collect();
    let mut tags: HashMap<u32, Vec<u32>> = HashMap::new();
    for (cn, tag) in class_name_to_tag {
        if let Some(sid) = sid_of(cn) {
            tags.entry(sid).or_default().push(*tag);
        }
    }
    for (sid, tag) in anon_stamp_pool.borrow().sid_to_tag_iter() {
        if !class_sids.contains(&sid.0) {
            tags.entry(sid.0).or_insert_with(|| vec![0]).push(tag);
        }
    }
    tags.values_mut().for_each(|t| t.sort_unstable());
    let mut hooks = Vec::new();
    for d in class_drops {
        let mut f = ssa::Function::new(&d.name, Type::Void);
        let p = f.add_param(Type::Ptr, "p");
        let prelude = f.add_block();
        let entry = f.add_block();
        let mut cx = Cx {
            layouts: struct_layouts,
            arr_layouts,
            class_sids: &class_sids,
            tags: &tags,
            drop_fids: &drop_fids,
            intr: intrinsics,
            prelude,
            cold: RefCell::new(Vec::new()),
            worklist: None,
            hooks: RefCell::new(Vec::new()),
        };
        let (sid, p) = (d.sid.0, Operand::Value(p));
        let mut stack = Vec::new();
        if worklist::holds_own_sid(&cx, sid) {
            let is_class = class_sids.contains(&sid);
            cx.worklist = Some(worklist::alloc(&mut f, prelude, sid, d.fid, p, is_class));
            cx.hook(sid, Test::UndefRoot, prelude);
            let wl = cx.worklist.as_ref().expect("just set");
            worklist::emit_loop(&mut f, wl, entry, |f, head, c, pop| {
                emit_release(f, &cx, head, c, sid, &mut stack, Some(pop))
            });
        } else {
            let after = emit_release(&mut f, &cx, entry, p, sid, &mut stack, None);
            f.set_term(after, Terminator::Ret(None));
        }
        for leg in cx.cold.take() {
            leg(&mut f);
        }
        f.set_term(prelude, Terminator::Br(entry));
        module.funcs[d.fid.0 as usize] = f;
        let made = cx.hooks.take().into_iter();
        hooks.extend(made.map(|(sid, test, blk)| Hook {
            fid: d.fid,
            sid,
            test,
            blk,
        }));
    }
    DropHooks {
        hooks,
        tags,
        class_sids,
    }
}
