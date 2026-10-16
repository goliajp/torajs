//! The instruction fusions of one function, planned once, and the
//! question the inst loop asks before emitting an instruction alone:
//! is it part of a fused group, and if so, does it emit that group or
//! is it absorbed into a neighbour? A constant address nothing reads
//! is answered here too: it emits nothing.

use torajs_core::ssa::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Block, Function, Inst, InstKind, ValueId};

use super::ModuleFacts;
use super::brfuse::{self, FusedCmp, FusedSelCmp};
use super::cmp;
use super::global_access::{self, GlobalAccesses};
use super::selinc::{self, SelectIncs};
use super::store_pair::{self, StorePair};
use super::throw_probe;
use super::throw_reg::Role;
use crate::regalloc::Assignment;
use crate::reloc::Reloc;

pub(super) struct Fusions {
    /// Block-tail single-use ICmp + CondBr pairs fuse into cmp+b.cond
    /// (or cbz/cbnz) — the inst loop skips the compare, the terminator
    /// emits it. See compile/brfuse.rs.
    cmps: HashMap<u32, FusedCmp>,
    select_cmps: HashMap<(u32, u32), FusedSelCmp>,
    /// `select c, (x+1), x` collapses into one CSINC, absorbing the ADD
    /// (and the ICmp behind it, which the ADD used to keep out of
    /// fusible_select_cmps' adjacency window). See compile/selinc.rs.
    select_incs: SelectIncs,
    /// Adjacent stores into neighbouring words, keyed by the first;
    /// the second is absorbed. See compile/store_pair.rs.
    store_pairs: HashMap<(u32, u32), StorePair>,
    /// `global_ref` / string / fn address materializations whose value
    /// no instruction or terminator reads (an inlined constructor's
    /// unused `new.target` name): an ADRP + ADD each, for nothing.
    dead_refs: HashSet<(u32, u32)>,
    /// 8-byte loads and stores through a symbol address nothing else
    /// reads, which fold the address's low half. See
    /// compile/global_access.rs.
    globals: GlobalAccesses,
    /// Blocks whose tail probe of the pending-throw flag reads x21
    /// instead, by the index of the first instruction it absorbs. See
    /// compile/throw_reg.rs.
    throw_probes: HashMap<u32, usize>,
}

impl Fusions {
    pub(super) fn of(
        func: &Function,
        alloc: &Assignment,
        facts: &ModuleFacts,
        role: Role,
        bit_tests: bool,
    ) -> Self {
        let uses = brfuse::count_uses(func);
        let throw_probes = if role.reads {
            throw_probe::probes(func, &facts.throw_reg, &uses)
        } else {
            HashMap::default()
        };
        Fusions {
            cmps: brfuse::fusible_cmps(func, bit_tests),
            select_cmps: brfuse::fusible_select_cmps(func, alloc),
            select_incs: selinc::fusible_select_incs(func),
            store_pairs: store_pair::plan(func, alloc),
            dead_refs: dead_refs(func, &uses),
            globals: global_access::plan(func, &uses, &facts.aligned8_syms),
            throw_probes,
        }
    }

    /// Whether the terminator of `block` branches on x21.
    pub(super) fn reads_throw_reg(&self, block: u32) -> bool {
        self.throw_probes.contains_key(&block)
    }

    /// The compare the terminator of `block` emits fused.
    pub(super) fn cmp_of(&self, block: u32) -> Option<&FusedCmp> {
        self.cmps.get(&block)
    }

    /// True when instruction `ii` of `block` was handled here (emitted
    /// as its fused group, or absorbed into one); false when the
    /// caller emits it on its own.
    pub(super) fn emit(
        &self,
        bytes: &mut Vec<u8>,
        relocs: &mut Vec<Reloc>,
        block: &Block,
        ii: usize,
        inst: &Inst,
        alloc: &Assignment,
    ) -> bool {
        let (b, i) = (block.id.0, ii as u32);
        if self.throw_probes.get(&b).is_some_and(|&first| ii >= first) {
            return true; // the terminator branches on x21 instead
        }
        if self.dead_refs.contains(&(b, i)) || self.globals.absorbed(b, i) {
            return true;
        }
        if let Some(sym) = self.globals.paged_sym(b, i) {
            global_access::emit_paged(bytes, relocs, inst, sym, alloc);
            return true;
        }
        if let Some(acc) = self.globals.access(b, i) {
            global_access::emit(bytes, relocs, inst, acc, alloc);
            return true;
        }
        if let Some(fc) = self.cmps.get(&b) {
            let tail = block.insts.len() - 1;
            if ii == tail || (fc.bits.is_some() && ii + 1 == tail) {
                return true; // the CondBr emits this compare (and AND) fused
            }
        }
        if self.select_cmps.contains_key(&(b, i)) {
            return true; // the next Select emits this compare fused
        }
        if self.select_incs.absorbed(b, i) {
            return true; // the CSINC downstream emits this add / compare
        }
        if let Some(plan) = self.select_incs.plan(b, i) {
            cmp::emit_select_inc(bytes, inst, plan, alloc);
            return true;
        }
        if let Some(pair) = self.store_pairs.get(&(b, i)) {
            store_pair::emit(bytes, pair, alloc);
            return true;
        }
        if ii > 0 && self.store_pairs.contains_key(&(b, i - 1)) {
            return true; // the store before emitted this one as its pair
        }
        if let InstKind::Select(ty, _, then_op, else_op) = &inst.kind
            && ii > 0
            && let Some(fc) = self.select_cmps.get(&(b, i - 1))
        {
            cmp::emit_select_fused(bytes, inst, ty, fc, then_op, else_op, alloc);
            return true;
        }
        false
    }
}

fn dead_refs(func: &Function, uses: &HashMap<ValueId, u32>) -> HashSet<(u32, u32)> {
    let mut out = HashSet::default();
    for block in &func.blocks {
        for (ii, inst) in block.insts.iter().enumerate() {
            let is_ref = matches!(
                inst.kind,
                InstKind::GlobalRef(_)
                    | InstKind::StringRef(_)
                    | InstKind::StaticStrRef(_)
                    | InstKind::FnAddr(_)
                    | InstKind::BoxedEntryAddr(_)
            );
            if is_ref && inst.result.is_some_and(|r| !uses.contains_key(&r)) {
                out.insert((block.id.0, ii as u32));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::compile_function;
    use torajs_core::ssa::{Function, InstKind, Operand, Terminator, Type};

    /// ADRP, any register and page.
    fn is_adrp(w: u32) -> bool {
        w & 0x9F00_0000 == 0x9000_0000
    }

    fn with_ref(read: bool) -> Function {
        let mut f = Function::new("t", Type::I64);
        let b = f.add_block();
        let g = f.append_inst(b, InstKind::GlobalRef("g".into()), Type::Ptr, None);
        let ret = if read {
            Operand::Value(f.append_inst(
                b,
                InstKind::Load(Type::I64, Operand::Value(g), 0),
                Type::I64,
                None,
            ))
        } else {
            Operand::ConstI64(0)
        };
        f.set_term(b, Terminator::Ret(Some(ret)));
        f
    }

    #[test]
    fn an_address_nothing_reads_emits_nothing() {
        let words = |f: &Function| -> Vec<u32> {
            compile_function(f)
                .bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect()
        };
        assert!(!words(&with_ref(false)).into_iter().any(is_adrp));
        assert!(words(&with_ref(true)).into_iter().any(is_adrp));
    }
}
