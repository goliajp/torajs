//! Filling the `__drop_<C>` bodies (`class_drop`) once every layout is
//! final, and the per-cell tests in them that a whole-program proof
//! may settle later ([`DropHooks`]).
//!
//! A release fn tests every cell it reaches against what a slot of the
//! sid could hold: a cell with an expando bag, a cell a WeakRef
//! observes. Which of those a
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
use crate::ssa_lower_emit_drop_value::class_drop::{Cx, emit_release};
use crate::ssa_lower_emit_drop_value::class_drop_setup::ClassDropFn;
use crate::ssa_lower_emit_drop_value::class_drop_worklist as worklist;

/// A per-cell test of a release fn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Test {
    /// Something observes some cell through a WeakRef.
    WeakRef,
    /// The cell carries an expando bag.
    Props,
}

/// One test: the fn, the sid of the cell tested, and the block whose
/// conditional branch the test feeds; a proven test always answers no.
pub(crate) struct Hook {
    pub fid: FuncId,
    pub sid: u32,
    pub test: Test,
    pub blk: BlockId,
}

/// Every test the release fns made, with what the proofs read about
/// the sids: which sids are class instances.
pub(crate) struct DropHooks {
    pub hooks: Vec<Hook>,
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
            if let Terminator::CondBr { cond, .. } = &mut f.blocks[h.blk.0 as usize].term {
                *cond = Operand::ConstBool(false);
                n += 1;
                fids.push(h.fid);
            }
        }
        fids.dedup();
        for fid in fids {
            sweep(&mut module.funcs[fid.0 as usize]);
        }
        n
    }
}

/// Drop what a fixed branch left without a use: the test's compare and
/// the loads and addresses that fed it. A release fn is not optimized
/// further, so nothing else would.
fn sweep(f: &mut ssa::Function) {
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
                    ssa::InstKind::ICmp(..) | ssa::InstKind::Load(..) | ssa::InstKind::GlobalRef(_)
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
    DropHooks { hooks, class_sids }
}
