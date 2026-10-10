//! Applying an `rc_borrow` plan: deletions, insertions after a def,
//! a release turned into a flag-guarded branch, and the retain /
//! release pair around a wrapped call. Sites are indices into the
//! function before any edit; every block is rebuilt in one pass so
//! they stay valid, and the blocks a split creates go at the end.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Function, Inst, InstKind, Operand, Terminator, ValueId};

use crate::cfg_edit::new_block;
use crate::rc_borrow::Site;

#[derive(Default)]
pub(crate) struct Plan {
    pub(crate) remove: Vec<Site>,
    pub(crate) after: Vec<(Site, Inst)>,
    /// A release to run only when the flag is set.
    pub(crate) cond_decs: Vec<(Site, ValueId)>,
    /// A call to bracket with a retain / release of the holder.
    pub(crate) wraps: Vec<(Site, ValueId)>,
    pub(crate) incs_elided: u32,
}

pub(crate) fn apply(func: &mut Function, plan: Plan) {
    let remove: HashSet<Site> = plan.remove.into_iter().collect();
    let mut after: HashMap<Site, Vec<Inst>> = HashMap::default();
    for (site, inst) in plan.after {
        after.entry(site).or_default().push(inst);
    }
    let cond_decs: HashMap<Site, ValueId> = plan.cond_decs.into_iter().collect();
    let wraps: HashMap<Site, ValueId> = plan.wraps.into_iter().collect();
    for b in 0..func.blocks.len() {
        let old = std::mem::take(&mut func.blocks[b].insts);
        let term = func.blocks[b].term.clone();
        let mut cur = b;
        let mut out: Vec<Inst> = Vec::with_capacity(old.len());
        for (i, inst) in old.into_iter().enumerate() {
            let site = (b, i);
            if remove.contains(&site) {
                continue;
            }
            if let Some(&flag) = cond_decs.get(&site) {
                let dec_blk = new_block(func);
                let cont = new_block(func);
                func.blocks[dec_blk.0 as usize].insts.push(inst);
                func.blocks[dec_blk.0 as usize].term = Terminator::Br(cont);
                func.blocks[cur].insts = std::mem::take(&mut out);
                func.blocks[cur].term = Terminator::CondBr {
                    cond: Operand::Value(flag),
                    then_blk: dec_blk,
                    else_blk: cont,
                };
                cur = cont.0 as usize;
                continue;
            }
            match wraps.get(&site) {
                Some(&m) => {
                    out.push(rc_inst(InstKind::RcIncAny(Operand::Value(m)), &inst));
                    out.push(inst.clone());
                    out.push(rc_inst(InstKind::RcDecAny(Operand::Value(m)), &inst));
                }
                None => out.push(inst),
            }
            if let Some(extra) = after.remove(&site) {
                out.extend(extra);
            }
        }
        func.blocks[cur].insts = out;
        func.blocks[cur].term = term;
    }
}

fn rc_inst(kind: InstKind, like: &Inst) -> Inst {
    Inst {
        result: None,
        kind,
        origin: like.origin,
    }
}
