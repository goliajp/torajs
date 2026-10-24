//! The module globals a cluster could keep as scalars: which value
//! names which global, and G1 / G3 over every function that runs.

use std::collections::{HashMap, HashSet};

use crate::ssa::{
    Function, InstKind, Module, Operand, PinScalar, Terminator, Type, ValueId, visit_value_operands,
};

use super::{Cand, ty, word};

/// The values of `f` that `global_ref` defines and nothing else does,
/// by symbol: a value with another def holds some other address on the
/// path through that def.
pub(super) fn global_refs(f: &Function) -> HashMap<ValueId, &str> {
    let mut refs = refs_of(f);
    if !refs.is_empty() {
        let again = redefined(f, &refs);
        refs.retain(|r, _| !again.contains(r));
    }
    refs
}

/// Every value of `f` that `global_ref` defines, by symbol.
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
    let mut seen: HashSet<ValueId> = HashSet::new();
    (f.blocks.iter().flat_map(|b| &b.insts))
        .filter_map(|i| i.result.filter(|r| refs.contains_key(r)))
        .filter(|r| !seen.insert(*r))
        .collect()
}

/// The global a load or a store of `kind` accesses through one of
/// `refs`.
pub(super) fn global_access<'r>(
    refs: &HashMap<ValueId, &'r str>,
    kind: &InstKind,
) -> Option<&'r str> {
    match kind {
        InstKind::Load(_, Operand::Value(p), _) | InstKind::Store(_, Operand::Value(p), _) => {
            refs.get(p).copied()
        }
        _ => None,
    }
}

/// G1 and G3 for each global of `cands`, over every function that
/// runs; a global that holds gets its type.
pub(super) fn globals_held(module: &Module, skip: &[bool], cands: &mut [Cand]) {
    for c in cands.iter_mut().filter(|c| c.no.is_none()) {
        let PinScalar::Global { sym, ty: held } = &mut c.scalar else {
            continue;
        };
        let decl = module.data_globals.iter().find(|g| g.name == *sym);
        let Some(t) = decl.map(|g| g.ty).filter(|t| word(*t)) else {
            c.no = Some("not a global of one integer word".into());
            continue;
        };
        let mut run = (module.funcs.iter().enumerate()).filter(|(i, _)| !skip[*i]);
        c.no = run.find_map(|(_, f)| global_escapes(f, sym, t));
        *held = t;
    }
}

/// Why `f` takes global `sym` of type `t` out of G1 or G3. A
/// `global_ref` of `sym` whose result has another def is an address
/// that escapes.
fn global_escapes(f: &Function, sym: &str, t: Type) -> Option<String> {
    let mut refs = refs_of(f);
    refs.retain(|_, s| *s == sym);
    if refs.is_empty() {
        return None;
    }
    if !redefined(f, &refs).is_empty() {
        return Some(format!("its address escapes in {}", f.name));
    }
    let refs: HashSet<ValueId> = refs.into_keys().collect();
    let held = |op: &Operand| matches!(op, Operand::Value(v) if refs.contains(v));
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        let moved = match &inst.kind {
            InstKind::Load(lt, p, 0) if held(p) => Some(*lt),
            InstKind::Store(v, p, 0) if held(p) && !held(v) => Some(ty(f, v)),
            kind => {
                let mut escapes = false;
                visit_value_operands(kind, |v| escapes |= refs.contains(&v));
                if escapes {
                    return Some(format!("its address escapes in {}", f.name));
                }
                None
            }
        };
        if moved.is_some_and(|m| m != t) {
            return Some(format!("accessed as another type in {}", f.name));
        }
    }
    let term_uses = f.blocks.iter().any(|b| match &b.term {
        Terminator::CondBr { cond, .. } => held(cond),
        Terminator::Ret(Some(v)) => held(v),
        _ => false,
    });
    term_uses.then(|| format!("its address escapes in {}", f.name))
}
