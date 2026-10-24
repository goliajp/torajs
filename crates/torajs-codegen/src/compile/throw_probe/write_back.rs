//! The write-back a cluster entry makes of its scalars after it calls
//! its body, which a probe of the throw flag may look past.

use torajs_core::ssa::id_hash::{HashMap, HashSet};
use torajs_core::ssa::{Function, InstKind, Operand, PinWord, ValueId};

/// Where an entry loaded a scalar from: the field at an offset of its
/// receiver, or a module global.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    Field(u64),
    Global(String),
}

/// A cluster entry's write-back of its scalars in `func`, which neither
/// calls nor reads x21: the `pinned` read of each scalar, with the word
/// the entry loaded the scalar from, and the addresses of those globals.
pub(super) struct WriteBack {
    this: Option<ValueId>,
    reads: HashMap<ValueId, Slot>,
    globals: HashSet<String>,
    refs: HashMap<ValueId, String>,
}

impl WriteBack {
    pub(super) fn of(func: &Function) -> Self {
        let this = func.params.first().copied();
        let insts = || func.blocks.iter().flat_map(|b| &b.insts);
        if !insts().any(|i| matches!(i.kind, InstKind::PinSet(..))) {
            return WriteBack {
                this,
                reads: HashMap::default(),
                globals: HashSet::default(),
                refs: HashMap::default(),
            };
        }
        let mut all_refs: HashMap<ValueId, &String> = (insts())
            .filter_map(|i| match (&i.kind, i.result) {
                (InstKind::GlobalRef(sym), Some(r)) => Some((r, sym)),
                _ => None,
            })
            .collect();
        // an address value with another def is not the global's on
        // every path, so a store through it is no write-back
        let mut seen: HashSet<ValueId> = HashSet::default();
        let again: HashSet<ValueId> = (insts())
            .filter_map(|i| i.result.filter(|r| all_refs.contains_key(r)))
            .filter(|r| !seen.insert(*r))
            .collect();
        all_refs.retain(|r, _| !again.contains(r));
        let loaded: HashMap<ValueId, Slot> = (insts())
            .filter_map(|i| match (&i.kind, i.result) {
                (InstKind::Load(_, Operand::Value(b), off), Some(r)) if Some(*b) == this => {
                    Some((r, Slot::Field(*off)))
                }
                (InstKind::Load(_, Operand::Value(b), 0), Some(r)) => {
                    all_refs.get(b).map(|s| (r, Slot::Global((*s).clone())))
                }
                _ => None,
            })
            .collect();
        let slots: HashMap<u8, &Slot> = (insts())
            .filter_map(|i| match &i.kind {
                InstKind::PinSet(PinWord::Scalar(k), Operand::Value(l)) => {
                    loaded.get(l).map(|s| (*k, s))
                }
                _ => None,
            })
            .collect();
        let reads = (insts())
            .filter_map(|i| match (&i.kind, i.result) {
                (InstKind::Pinned(PinWord::Scalar(k)), Some(r)) => {
                    slots.get(k).map(|s| (r, (*s).clone()))
                }
                _ => None,
            })
            .collect();
        let globals: HashSet<String> = (slots.values())
            .filter_map(|s| match s {
                Slot::Global(sym) => Some(sym.clone()),
                Slot::Field(_) => None,
            })
            .collect();
        let refs = (all_refs.into_iter())
            .filter(|(_, s)| globals.contains(*s))
            .map(|(r, s)| (r, s.clone()))
            .collect();
        WriteBack {
            this,
            reads,
            globals,
            refs,
        }
    }

    /// Whether `kind` is part of the write-back: a `pinned` read of a
    /// scalar, the address of a global the entry loaded one from, or the
    /// store of a scalar into the word it was loaded from.
    pub(super) fn contains(&self, kind: &InstKind) -> bool {
        match kind {
            InstKind::Pinned(PinWord::Scalar(_)) => true,
            InstKind::GlobalRef(sym) => self.globals.contains(sym),
            InstKind::Store(Operand::Value(v), Operand::Value(b), off) => match self.reads.get(v) {
                Some(Slot::Field(o)) => Some(*b) == self.this && o == off,
                Some(Slot::Global(sym)) => *off == 0 && self.refs.get(b) == Some(sym),
                None => false,
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ProbedCalls;
    use torajs_core::ssa::{
        BlockId, FuncId, Function, IPred, InstKind, Operand, PinWord, StructId, THROW_ACTIVE_SYM,
        Terminator, Type, ValueId,
    };

    fn v(x: ValueId) -> Operand {
        Operand::Value(x)
    }

    /// An entry that loads global `nodes` into scalar 0, calls its body,
    /// stores the scalar back into `back` (a global) and probes the
    /// throw flag.
    fn entry(back: &str) -> Function {
        let mut f = Function::new("__cm_P__e", Type::I64);
        let p = f.add_param(Type::Obj(StructId(0)), "p");
        let (b0, ok, thrown) = (f.add_block(), f.add_block(), f.add_block());
        let g = f.append_inst(b0, InstKind::GlobalRef("nodes".into()), Type::Ptr, None);
        let l = f.append_inst(b0, InstKind::Load(Type::I64, v(g), 0), Type::I64, None);
        f.append_void(b0, InstKind::PinSet(PinWord::Scalar(0), v(l)));
        let r = f.append_inst(b0, InstKind::Call(FuncId(1), vec![v(p)]), Type::I64, None);
        let q = f.append_inst(b0, InstKind::Pinned(PinWord::Scalar(0)), Type::I64, None);
        let h = f.append_inst(b0, InstKind::GlobalRef(back.into()), Type::Ptr, None);
        f.append_void(b0, InstKind::Store(v(q), v(h), 0));
        let flag = InstKind::GlobalRef(THROW_ACTIVE_SYM.into());
        let flag = f.append_inst(b0, flag, Type::Ptr, None);
        let w = f.append_inst(b0, InstKind::Load(Type::I64, v(flag), 0), Type::I64, None);
        let c = InstKind::ICmp(IPred::Ne, v(w), Operand::ConstI64(0));
        let c = f.append_inst(b0, c, Type::Bool, None);
        let br = Terminator::CondBr {
            cond: v(c),
            then_blk: thrown,
            else_blk: ok,
        };
        f.set_term(b0, br);
        f.set_term(ok, Terminator::Ret(Some(v(r))));
        f.append_void(thrown, InstKind::ThrowSignal);
        f.set_term(thrown, Terminator::Ret(Some(Operand::ConstI64(0))));
        f
    }

    fn call_probed(f: &Function) -> bool {
        let b = &f.blocks[BlockId(0).0 as usize];
        let at = (b.insts.iter())
            .position(|i| matches!(i.kind, InstKind::Call(..)))
            .unwrap();
        ProbedCalls::of(f).probed(b, at)
    }

    #[test]
    fn a_probe_looks_past_the_store_back_of_a_global_scalar() {
        assert!(call_probed(&entry("nodes")));
    }

    #[test]
    fn a_store_into_another_global_after_the_call_is_not_a_write_back() {
        assert!(!call_probed(&entry("other")));
    }
}
