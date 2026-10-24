//! Which values of a member are its receiver, and which accesses
//! reach a scalar's field on some other cell of the class.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{FuncId, Function, InstKind, Operand, StructId, Type, ValueId};

use super::super::aliases;
use super::def_counts;

/// The values of `f` that hold its receiver on every path: the
/// receiver, and each value all of whose defs copy one of them (as
/// `mem2reg` leaves a local the program sets to `this` on each path).
/// Only values a chain of copies reaches from the receiver are asked,
/// so copies of one another that never start from it are not taken.
/// Empty when something redefines the receiver itself.
pub(super) fn receivers(f: &Function) -> HashSet<ValueId> {
    let this = f.params[0];
    let mut srcs: HashMap<ValueId, Vec<Option<ValueId>>> = HashMap::default();
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        let Some(r) = inst.result else { continue };
        let src = match &inst.kind {
            InstKind::Identity(Operand::Value(x)) | InstKind::Copy(_, Operand::Value(x)) => {
                Some(*x)
            }
            _ => None,
        };
        srcs.entry(r).or_default().push(src);
    }
    if srcs.contains_key(&this) {
        return HashSet::default();
    }
    let copies = |s: &[Option<ValueId>]| s.iter().all(Option::is_some);
    let mut set: HashSet<ValueId> = [this].into_iter().collect();
    loop {
        let reached: Vec<ValueId> = (srcs.iter())
            .filter(|(r, s)| !set.contains(*r) && copies(s))
            .filter(|(_, s)| s.iter().flatten().any(|x| set.contains(x)))
            .map(|(r, _)| *r)
            .collect();
        if reached.is_empty() {
            break;
        }
        set.extend(reached);
    }
    loop {
        let out: Vec<ValueId> = (set.iter())
            .filter(|r| **r != this)
            .filter(|r| srcs[*r].iter().flatten().any(|x| !set.contains(x)))
            .copied()
            .collect();
        if out.is_empty() {
            return set;
        }
        for r in out {
            set.remove(&r);
        }
    }
}

/// Whether member `f` reaches a scalar of `fields` on a cell of class
/// sid `sid` that is not a cell it allocated itself (a call to
/// `alloc`, defined once): a load or store at a scalar's offset on a
/// cell other than its receiver (`recv`), or an access at a computed
/// offset on any such cell. The body rewrite would leave that access
/// in memory while the register holds the scalar.
pub(super) fn foreign(
    f: &Function,
    sid: StructId,
    fields: &[(u64, Type)],
    recv: &HashSet<ValueId>,
    alloc: Option<FuncId>,
) -> bool {
    let defs = def_counts(f);
    let fresh: HashSet<ValueId> = (f.blocks.iter().flat_map(|b| &b.insts))
        .filter(|i| matches!(i.kind, InstKind::Call(t, _) if Some(t) == alloc))
        .filter_map(|i| i.result)
        .filter(|r| defs.get(r) == Some(&1))
        .collect();
    let alias = aliases(f);
    (f.blocks.iter().flat_map(|b| &b.insts)).any(|i| {
        let (b, off) = match &i.kind {
            InstKind::Load(_, Operand::Value(b), off)
            | InstKind::Store(_, Operand::Value(b), off) => (b, Some(*off)),
            InstKind::LoadDyn(_, Operand::Value(b), _)
            | InstKind::StoreDyn(_, Operand::Value(b), _)
            | InstKind::LoadDynScaled8(_, Operand::Value(b), _)
            | InstKind::StoreDynScaled8(_, Operand::Value(b), _)
            | InstKind::LoadU8Dyn(Operand::Value(b), _)
            | InstKind::LoadU16Dyn(Operand::Value(b), _) => (b, None),
            _ => return false,
        };
        let cell = matches!(f.value_type(*b), Type::Obj(s) | Type::Inline(s) if s == sid);
        let hits = match off {
            Some(off) => fields.iter().any(|(o, _)| *o == off) && !recv.contains(b),
            None => true,
        };
        cell && hits && !fresh.contains(alias.get(b).unwrap_or(b))
    })
}
