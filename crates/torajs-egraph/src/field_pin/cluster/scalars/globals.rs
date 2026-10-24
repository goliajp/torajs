//! The census (pass 2.5) judged the cluster's scalars on the module as
//! it was then; inlining and cloning since may have made functions that
//! name a global or call a member. So the cluster pass asks again, on
//! the module it rewrites, what keeping a global in a register and
//! writing it back at the entries needs: nothing the members reach
//! enters the cluster again, nothing they reach but themselves names
//! the global, and the global's address is only ever the base of a load
//! or a store of its one type, through a value nothing else defines.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    FuncId, Function, InstKind, Module, Operand, Terminator, Type, ValueId, visit_value_operands,
};

use super::super::{Receivers, passes_own};
use super::Slot;

/// The functions with a body the members reach by direct calls,
/// themselves included.
fn reached(module: &Module, members: &[FuncId]) -> Vec<FuncId> {
    let mut seen: HashSet<FuncId> = members.iter().copied().collect();
    let mut out = members.to_vec();
    let mut i = 0;
    while let Some(&fid) = out.get(i) {
        i += 1;
        let f = &module.funcs[fid.0 as usize];
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            if let InstKind::Call(t, _) = &inst.kind
                && !module.funcs[t.0 as usize].is_declaration()
                && seen.insert(*t)
            {
                out.push(*t);
            }
        }
    }
    out
}

/// Whether something the members reach (but a function that never
/// runs, `quiet`) calls a member from outside the cluster, calls one
/// from inside with a value not known to be its own receiver (a call
/// `retarget` leaves on the entry), or takes a member's address: the
/// member entered again would load the scalars from memory while the
/// one below it holds them in the registers.
pub(super) fn reentered(
    module: &Module,
    members: &[FuncId],
    quiet: &HashSet<FuncId>,
    recv: &Receivers,
) -> bool {
    (reached(module, members).iter())
        .filter(|g| !quiet.contains(g))
        .any(|g| {
            let f = &module.funcs[g.0 as usize];
            let own = recv.get(g);
            (f.blocks.iter().flat_map(|b| &b.insts)).any(|i| match &i.kind {
                InstKind::Call(t, args) => {
                    members.contains(t) && !own.is_some_and(|r| passes_own(r, args))
                }
                InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) => members.contains(t),
                _ => false,
            })
        })
}

/// Whether the members may keep global `sym` of type `ty` in a
/// register: no function they reach but themselves names it, and in
/// every function but those of `quiet` (the functions that never run
/// and call a member or take its address; the others that never run
/// are asked too) its address is the base of loads and stores of `ty`
/// at offset 0 and nothing else.
pub(super) fn kept(
    module: &Module,
    members: &[FuncId],
    quiet: &HashSet<FuncId>,
    sym: &str,
    ty: Type,
) -> bool {
    let names = |f: &Function| (global_refs(f).values()).any(|s| *s == sym);
    let outside = (reached(module, members).iter())
        .filter(|g| !members.contains(g) && !quiet.contains(g))
        .any(|g| names(&module.funcs[g.0 as usize]));
    let asked =
        (module.funcs.iter().enumerate()).filter(|(i, _)| !quiet.contains(&FuncId(*i as u32)));
    !outside && asked.into_iter().all(|(_, f)| only_accessed(f, sym, ty))
}

/// Whether every use in `f` of an address of global `sym` is the base
/// of a load or a store of `ty` at offset 0. A `global_ref` of `sym`
/// whose result has another def is an address that escapes.
fn only_accessed(f: &Function, sym: &str, ty: Type) -> bool {
    let mut refs = refs_of(f);
    refs.retain(|_, s| *s == sym);
    if refs.is_empty() {
        return true;
    }
    if !redefined(f, &refs).is_empty() {
        return false;
    }
    let refs: HashSet<ValueId> = refs.into_keys().collect();
    let addr = |op: &Operand| matches!(op, Operand::Value(v) if refs.contains(v));
    let insts = (f.blocks.iter().flat_map(|b| &b.insts)).all(|i| match &i.kind {
        InstKind::Load(t, p, 0) if addr(p) => *t == ty,
        InstKind::Store(v, p, 0) if addr(p) => !addr(v) && operand_type(f, v) == ty,
        kind => {
            let mut escapes = false;
            visit_value_operands(kind, |v| escapes |= refs.contains(&v));
            !escapes
        }
    });
    let terms = f.blocks.iter().all(|b| match &b.term {
        Terminator::CondBr { cond, .. } => !addr(cond),
        Terminator::Ret(Some(v)) => !addr(v),
        _ => true,
    });
    insts && terms
}

fn operand_type(f: &Function, op: &Operand) -> Type {
    match op {
        Operand::Value(v) => f.value_type(*v),
        Operand::ConstI64(_) => Type::I64,
        Operand::ConstI32(_) => Type::I32,
        Operand::ConstF64(_) => Type::F64,
        Operand::ConstBool(_) => Type::Bool,
        Operand::ConstPtrNull => Type::Ptr,
    }
}

/// The values of `f` a `global_ref` defines and nothing else does,
/// with the global: a value with another def holds some other address
/// on the path through that def.
pub(super) fn global_refs(f: &Function) -> HashMap<ValueId, &str> {
    let mut refs = refs_of(f);
    if !refs.is_empty() {
        let again = redefined(f, &refs);
        refs.retain(|r, _| !again.contains(r));
    }
    refs
}

/// Every value of `f` a `global_ref` defines, with the global.
fn refs_of(f: &Function) -> HashMap<ValueId, &str> {
    (f.blocks.iter().flat_map(|b| &b.insts))
        .filter_map(|i| match (&i.kind, i.result) {
            (InstKind::GlobalRef(sym), Some(r)) => Some((r, sym.as_str())),
            _ => None,
        })
        .collect()
}

/// The values of `refs` that `f` defines more than once.
fn redefined(f: &Function, refs: &HashMap<ValueId, &str>) -> HashSet<ValueId> {
    let mut seen: HashSet<ValueId> = HashSet::default();
    (f.blocks.iter().flat_map(|b| &b.insts))
        .filter_map(|i| i.result.filter(|r| refs.contains_key(r)))
        .filter(|r| !seen.insert(*r))
        .collect()
}

/// Remove each `global_ref` of a global of `held` whose result nothing
/// reads any longer.
pub(super) fn drop_unread_refs(f: &mut Function, held: &[(Slot, Type)]) {
    let syms: HashSet<&str> = (held.iter())
        .filter_map(|(s, _)| match s {
            Slot::Global(sym) => Some(sym.as_str()),
            Slot::Field(_) => None,
        })
        .collect();
    if syms.is_empty() {
        return;
    }
    let mut read: HashSet<ValueId> = HashSet::default();
    for block in &f.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                read.insert(v);
            });
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = &block.term
        {
            read.insert(*v);
        }
    }
    for block in &mut f.blocks {
        block.insts.retain(|i| match (&i.kind, i.result) {
            (InstKind::GlobalRef(s), Some(r)) => !(syms.contains(s.as_str()) && !read.contains(&r)),
            _ => true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::{body, entry, receivers};
    use super::*;
    use torajs_core::ssa::{BinOp, BlockId, Inst, PinWord, StructId};

    const SYM: &str = "nodes";
    const MEMBER: FuncId = FuncId(0);
    const HELPER: FuncId = FuncId(1);

    fn v(x: ValueId) -> Operand {
        Operand::Value(x)
    }

    fn held() -> [(Slot, Type); 1] {
        [(Slot::Global(SYM.into()), Type::I64)]
    }

    /// `nodes = nodes + 1` through one `global_ref`.
    fn bump(f: &mut Function, b: BlockId) {
        let g = f.append_inst(b, InstKind::GlobalRef(SYM.into()), Type::Ptr, None);
        let n = f.append_inst(b, InstKind::Load(Type::I64, v(g), 0), Type::I64, None);
        let add = InstKind::BinOp(BinOp::Add, v(n), Operand::ConstI64(1));
        let a = f.append_inst(b, add, Type::I64, None);
        f.append_void(b, InstKind::Store(v(a), v(g), 0));
    }

    /// A member that bumps the global and calls `HELPER`, whose block
    /// `helper` fills.
    fn module(helper: impl Fn(&mut Function, BlockId)) -> Module {
        let mut m = Function::new("__cm_P__e", Type::Void);
        let this = m.add_param(Type::Obj(StructId(0)), "this");
        let b = m.add_block();
        bump(&mut m, b);
        m.append_void(b, InstKind::Call(HELPER, vec![v(this)]));
        m.set_term(b, Terminator::Ret(None));
        let mut h = Function::new("helper", Type::Void);
        h.add_param(Type::Obj(StructId(0)), "p");
        let hb = h.add_block();
        helper(&mut h, hb);
        h.set_term(hb, Terminator::Ret(None));
        Module {
            funcs: vec![m, h],
            ..Default::default()
        }
    }

    fn none() -> HashSet<FuncId> {
        HashSet::default()
    }

    fn own(m: &Module) -> Receivers {
        [(MEMBER, receivers(&m.funcs[0]))].into_iter().collect()
    }

    #[test]
    fn a_global_only_the_members_access_is_kept() {
        let m = module(|_, _| {});
        assert!(!reentered(&m, &[MEMBER], &none(), &own(&m)));
        assert!(kept(&m, &[MEMBER], &none(), SYM, Type::I64));
        assert!(!kept(&m, &[MEMBER], &none(), SYM, Type::I32));
    }

    #[test]
    fn a_global_a_function_the_members_reach_names_stays_in_memory() {
        let m = module(|f, b| {
            f.append_inst(b, InstKind::GlobalRef(SYM.into()), Type::Ptr, None);
        });
        assert!(!kept(&m, &[MEMBER], &none(), SYM, Type::I64));
        // unless that function never runs
        let quiet: HashSet<FuncId> = [HELPER].into_iter().collect();
        assert!(kept(&m, &[MEMBER], &quiet, SYM, Type::I64));
    }

    #[test]
    fn a_global_whose_address_goes_anywhere_but_a_load_or_store_base_stays_in_memory() {
        // stored into a cell, in a function the members do not reach
        let mut m = module(|_, _| {});
        let mut main = Function::new("main", Type::Void);
        let p = main.add_param(Type::Obj(StructId(0)), "p");
        let b = main.add_block();
        let g = main.append_inst(b, InstKind::GlobalRef(SYM.into()), Type::Ptr, None);
        main.append_void(b, InstKind::Store(v(g), v(p), 40));
        main.set_term(b, Terminator::Ret(None));
        m.funcs.push(main);
        assert!(!kept(&m, &[MEMBER], &none(), SYM, Type::I64));
        // read at a nonzero offset
        let mut m = module(|_, _| {});
        let f = &mut m.funcs[0];
        let g = f.insert_inst(BlockId(0), 0, InstKind::GlobalRef(SYM.into()), Type::Ptr);
        f.insert_inst(BlockId(0), 1, InstKind::Load(Type::I64, v(g), 8), Type::I64);
        assert!(!kept(&m, &[MEMBER], &none(), SYM, Type::I64));
    }

    #[test]
    fn a_global_address_value_with_another_def_stays_in_memory() {
        let mut m = module(|_, _| {});
        let f = &mut m.funcs[0];
        let g = f.blocks[0].insts[0].result.unwrap();
        f.blocks[0].insts.push(Inst {
            result: Some(g),
            kind: InstKind::Identity(Operand::ConstPtrNull),
            origin: None,
        });
        assert!(global_refs(f).is_empty());
        assert!(!kept(&m, &[MEMBER], &none(), SYM, Type::I64));
    }

    #[test]
    fn a_member_calling_a_member_with_a_value_not_its_receiver_enters_again() {
        let mut m = module(|_, _| {});
        let f = &mut m.funcs[0];
        let this = f.params[0];
        let b = BlockId(0);
        let other = f.append_inst(
            b,
            InstKind::Load(Type::Obj(StructId(0)), v(this), 40),
            Type::Obj(StructId(0)),
            None,
        );
        f.append_void(b, InstKind::Call(MEMBER, vec![v(other)]));
        assert!(reentered(&m, &[MEMBER], &none(), &own(&m)));
        // a value every def of which copies the receiver is the receiver
        let mut m = module(|_, _| {});
        let f = &mut m.funcs[0];
        let this = f.params[0];
        let cur = f.append_inst(
            b,
            InstKind::Copy(Type::Obj(StructId(0)), v(this)),
            Type::Obj(StructId(0)),
            None,
        );
        f.blocks[0].insts.push(Inst {
            result: Some(cur),
            kind: InstKind::Copy(Type::Obj(StructId(0)), v(this)),
            origin: None,
        });
        f.append_void(b, InstKind::Call(MEMBER, vec![v(cur)]));
        assert!(!reentered(&m, &[MEMBER], &none(), &own(&m)));
    }

    #[test]
    fn a_reached_function_calling_a_member_enters_the_cluster_again() {
        let m = module(|f, b| {
            let p = f.params[0];
            f.append_void(b, InstKind::Call(MEMBER, vec![v(p)]));
        });
        assert!(reentered(&m, &[MEMBER], &none(), &own(&m)));
        let m = module(|f, b| {
            f.append_inst(b, InstKind::FnAddr(MEMBER), Type::Ptr, None);
        });
        assert!(reentered(&m, &[MEMBER], &none(), &own(&m)));
    }

    #[test]
    fn a_body_reads_and_writes_the_global_in_its_register_and_drops_the_address() {
        let mut m = module(|_, _| {});
        let f = &mut m.funcs[0];
        let writes: HashSet<FuncId> = [MEMBER].into_iter().collect();
        let r = body(f, &held(), &writes, false);
        assert_eq!((r.accesses, r.copies), (2, 0));
        let k: Vec<&InstKind> = f.blocks[0].insts.iter().map(|i| &i.kind).collect();
        assert_eq!(k[0], &InstKind::Pinned(PinWord::Scalar(0)));
        assert!(matches!(k[1], InstKind::BinOp(BinOp::Add, ..)));
        assert!(matches!(k[2], InstKind::PinSet(PinWord::Scalar(0), _)));
        assert!(!k.iter().any(|k| matches!(k, InstKind::GlobalRef(_))));
    }

    #[test]
    fn strict_keeps_the_store_and_its_address_and_checks_the_read_against_memory() {
        let mut m = module(|_, _| {});
        let f = &mut m.funcs[0];
        body(f, &held(), &HashSet::default(), true);
        let head: Vec<&InstKind> = f.blocks[0].insts.iter().map(|i| &i.kind).collect();
        assert!(matches!(head[0], InstKind::GlobalRef(_)));
        assert_eq!(head[1], &InstKind::Pinned(PinWord::Scalar(0)));
        assert!(matches!(head[2], InstKind::GlobalRef(s) if s == SYM));
        assert!(matches!(head[3], InstKind::Load(Type::I64, _, 0)));
        assert!(matches!(head[4], InstKind::ICmp(..)));
        let rest = &f.blocks[2].insts;
        assert!(matches!(rest[1].kind, InstKind::PinSet(..)));
        assert!(matches!(rest[2].kind, InstKind::Store(_, _, 0)));
    }

    #[test]
    fn an_entry_loads_the_global_before_the_call_and_stores_it_back_after() {
        let mut f = Function::new("__cm_P__e", Type::Void);
        let this = f.add_param(Type::Obj(StructId(0)), "this");
        let b = f.add_block();
        f.append_void(b, InstKind::Call(MEMBER, vec![v(this)]));
        f.set_term(b, Terminator::Ret(None));
        entry(&mut f, &held());
        let k: Vec<&InstKind> = f.blocks[0].insts.iter().map(|i| &i.kind).collect();
        let res = |i: usize| f.blocks[0].insts[i].result.unwrap();
        assert!(matches!(k[0], InstKind::GlobalRef(s) if s == SYM));
        assert_eq!(k[1], &InstKind::Load(Type::I64, v(res(0)), 0));
        assert_eq!(k[2], &InstKind::PinSet(PinWord::Scalar(0), v(res(1))));
        assert!(matches!(k[3], InstKind::Call(..)));
        assert_eq!(k[4], &InstKind::Pinned(PinWord::Scalar(0)));
        assert!(matches!(k[5], InstKind::GlobalRef(s) if s == SYM));
        assert_eq!(k[6], &InstKind::Store(v(res(4)), v(res(5)), 0));
    }
}
