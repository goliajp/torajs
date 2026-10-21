//! Which per-cell tests of the `__drop_<C>` release fns the finished
//! module settles (`class_drop_fill::DropHooks`).
//!
//! The tag test hands a cell of another layout to the runtime's walk
//! over its own tag's row. A class sid is *closed* when every cell a
//! slot of it can hold is one its own factory made: no slot of it is
//! open to a cell of another layout (`cell_slot_flow`), no other layout
//! extends or repeats its fields (a subclass, a literal of its shape),
//! and every allocation typed as it is stamped with one of its tags,
//! none decoded out of bits or named as a global (the undefined
//! sentinel aside, which its own test still catches).
//!
//! A worklist release (a sid whose fields hold the sid) tests every
//! cell it takes for the undefined sentinel. A closed and sealed sid's
//! own-sid fields are written only by typed stores the module shows;
//! when none of them can store the sentinel (`undef`), a cell taken
//! from such a field never is one, and only the root is tested, once.
//!
//! The props test releases an expando bag, the WeakRef test tells the
//! registry a target died. Only the runtime gives a cell a bag or a
//! WeakRef, and only a cell that reached it as a value ([`exposed`]).
//! A class sid is *sealed* when no slot of it is open to a cell of
//! another layout (`cell_slot_flow`) and no layout its cells can be
//! read as (along the prefix paths `cell_slot_flow` records), nor one
//! whose cells can reach its slots, is exposed.
//!
//! A release fn buffers a class cell whose count stays above zero as
//! a cycle root and tests a dying one for the buffer's flag. A sid
//! whose cells no cycle can pass through, and that nothing but its
//! release fn could buffer ([`acyclic`]), needs neither.
//!
//! All of it is read over the functions that run (`asleep`): a class's
//! method rows are entered only once an instance or the class itself
//! became a value, which an exposed class instance, or a name that
//! reaches a class or a scope by a road no census follows, would make
//! possible; then nothing is settled. `TORAJS_DROP_PROOF_STATS=1` prints
//! the verdict per sid.

mod acyclic;
mod asleep;
mod exposed;
#[cfg(test)]
mod tests;
mod undef;

use std::collections::{BTreeSet, HashMap};

use crate::ast::{Ast, Expr};
use crate::ssa::{InstKind, Module, Operand, StructId, Type};
use crate::ssa_lower::OBJ_CLASS_TAG_OFF;
use crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM;
use crate::ssa_lower_emit_drop_value::class_drop_fill::{DropHooks, Test};

use super::cell_slot_flow::{EnvLayouts, is_prefix};

/// Names that reach a class, a prototype or a scope by a road the
/// SSA does not show.
const ROADS: &[&str] = &[
    "eval",
    "Function",
    "Reflect",
    "Proxy",
    "globalThis",
    "arguments",
];

const OBJ_ALLOC: &str = "__torajs_obj_alloc";

fn stats() -> bool {
    std::env::var_os("TORAJS_DROP_PROOF_STATS").is_some()
}

/// The functions only a class method row enters, when some test is
/// to be settled: none when no release fn made one, or when a name
/// wakes the rows (printed under the stats).
pub(crate) fn asleep(module: &Module, ast: &Ast, hooks: &DropHooks) -> Option<Vec<bool>> {
    if hooks.hooks.is_empty() {
        return None;
    }
    if let Some(why) = wakes_rows(ast) {
        if stats() {
            eprintln!("[drop_proof] none: {why}");
        }
        return None;
    }
    Some(asleep::row_only(module))
}

/// The open sids over the functions that run, their prefix paths, and
/// the functions ruled out (`cell_slot_flow::open_sids_both`).
pub(crate) type Flow = (Vec<bool>, Vec<(StructId, StructId)>, Vec<bool>);

/// Settle every test of `hooks` the module proves. Answers what it
/// proves of the class sids in `asked` as well (`pin_facts`); nothing
/// when an instance of some class reaches the runtime.
pub(crate) fn settle(
    module: &mut Module,
    envs: &EnvLayouts,
    hooks: &DropHooks,
    flow: Flow,
    asked: &[u32],
) -> HashMap<u32, Seal> {
    let verdicts = match verdicts(module, envs, hooks, flow, asked) {
        Ok(v) => v,
        Err(why) => {
            if stats() {
                eprintln!("[drop_proof] none: {why}");
            }
            return HashMap::new();
        }
    };
    let n = hooks.settle(module, |sid, test| {
        let v = &verdicts[&sid].0;
        match test {
            Test::Tag => v.seal.closed,
            Test::WeakRef | Test::Props => v.seal.sealed,
            Test::Undef | Test::UndefRoot => v.defined,
            Test::Buffered | Test::BufferLeg(_) => v.unbuffered,
        }
    });
    if stats() {
        let mut v: Vec<_> = (verdicts.iter())
            .filter(|(_, (_, hooked))| *hooked)
            .collect();
        v.sort_unstable_by_key(|(sid, _)| **sid);
        for (sid, (v, _)) in v {
            eprintln!(
                "[drop_proof] sid {sid}: sealed {} closed {} defined {} unbuffered {}",
                v.seal.sealed, v.seal.closed, v.defined, v.unbuffered
            );
        }
        eprintln!("[drop_proof] {n} of {} tests settled", hooks.hooks.len());
    }
    (verdicts.into_iter())
        .map(|(sid, (v, _))| (sid, v.seal))
        .collect()
}

/// What the module proves of a class sid's cells, for a census that
/// reads its fields across calls.
#[derive(Clone, Copy)]
pub(crate) struct Seal {
    pub(crate) sealed: bool,
    pub(crate) closed: bool,
    /// no slot of another layout reads its cells
    pub(crate) unwidened: bool,
}

/// What the module proves of one sid (`defined` and `unbuffered`
/// only of a hooked one).
struct Verdict {
    seal: Seal,
    /// its own-sid fields never hold the undefined sentinel
    defined: bool,
    /// nothing can buffer one of its cells as a cycle root
    unbuffered: bool,
}

fn verdicts(
    module: &Module,
    envs: &EnvLayouts,
    hooks: &DropHooks,
    (open, widens, skip): Flow,
    asked: &[u32],
) -> Result<HashMap<u32, (Verdict, bool)>, String> {
    let exposed = exposed::exposed_sids(module, &skip);
    let mut classes: Vec<u32> = hooks.class_sids.iter().copied().collect();
    classes.sort_unstable();
    if let Some(s) = classes.iter().find(|s| exposed[**s as usize]) {
        return Err(format!("an instance of class sid {s} reaches the runtime"));
    }
    let shared = std::cell::OnceCell::new();
    let shared = || shared.get_or_init(|| undef::Shared::new(module, &skip));
    let sids: BTreeSet<u32> = hooks.hooks.iter().map(|h| h.sid).collect();
    let mut out = HashMap::new();
    for sid in sids.iter().copied().chain(asked.iter().copied()) {
        if out.contains_key(&sid) {
            continue;
        }
        let hooked = sids.contains(&sid);
        let base = hooks.class_sids.contains(&sid) && !open[sid as usize];
        let sealed = base
            && reach(&widens, sid, false).iter().all(|w| {
                reach(&widens, *w, true)
                    .iter()
                    .all(|n| !exposed[*n as usize])
            });
        let tags = hooks.tags.get(&sid).map_or(&[][..], Vec::as_slice);
        let closed = base && !extended(module, sid) && born_own(module, shared(), sid, tags);
        let unwidened = widens.iter().all(|(w, _)| w.0 != sid);
        let quiet =
            hooked && sealed && closed && unwidened && widens.iter().all(|(_, n)| n.0 != sid);
        let defined = quiet
            && !undef::held_elsewhere(module, envs, sid)
            && undef::own_fields_defined(module, shared(), sid);
        let unbuffered = quiet && acyclic::unbuffered(module, shared(), envs, sid);
        let seal = Seal {
            sealed,
            closed,
            unwidened,
        };
        let v = Verdict {
            seal,
            defined,
            unbuffered,
        };
        out.insert(sid, (v, hooked));
    }
    Ok(out)
}

/// A name in the program that reaches a class or a scope as a value:
/// a road, a class named as a value, a `new` of a computed callee, or
/// `new.target`.
fn wakes_rows(ast: &Ast) -> Option<String> {
    if !ast.new_target_fns.is_empty() {
        return Some("`new.target`".into());
    }
    ast.exprs.iter().find_map(|e| match e {
        Expr::Ident(n) if ROADS.contains(&n.as_str()) => Some(format!("the road `{n}`")),
        Expr::Ident(n) if ast.class_parents.contains_key(n) => {
            Some(format!("the class `{n}` as a value"))
        }
        Expr::NewDynamic { .. } => Some("a `new` of a computed callee".into()),
        _ => None,
    })
}

/// `sid` and every sid a prefix path leads to from it (`down`: the
/// layouts its cells are read as) or from which one leads to it.
fn reach(widens: &[(StructId, StructId)], sid: u32, down: bool) -> Vec<u32> {
    let mut seen = vec![sid];
    let mut i = 0;
    while i < seen.len() {
        let at = seen[i];
        for (wide, narrow) in widens {
            let (from, to) = if down { (wide, narrow) } else { (narrow, wide) };
            if from.0 == at && !seen.contains(&to.0) {
                seen.push(to.0);
            }
        }
        i += 1;
    }
    seen
}

/// Some other layout extends `sid`'s by field name, or repeats it.
fn extended(module: &Module, sid: u32) -> bool {
    let layouts = &module.struct_layouts;
    let base = &layouts[sid as usize];
    (layouts.iter().enumerate()).any(|(i, l)| i != sid as usize && is_prefix(base, l))
}

/// Every cell typed `sid` that a running function makes is an
/// allocation stamped with one of `tags`.
fn born_own(module: &Module, shared: &undef::Shared, sid: u32, tags: &[u32]) -> bool {
    let alloc = module.funcs.iter().position(|f| f.name == OBJ_ALLOC);
    let own = Type::Obj(StructId(sid));
    for &i in shared.mentions(sid) {
        let f = &module.funcs[i];
        let insts = || f.blocks.iter().flat_map(|b| &b.insts);
        let stamped = |v| {
            insts().any(|inst| {
                matches!(&inst.kind, InstKind::Store(Operand::ConstI64(t), Operand::Value(p), OBJ_CLASS_TAG_OFF)
                    if *p == v && u32::try_from(*t).is_ok_and(|t| tags.contains(&t)))
            })
        };
        for inst in insts() {
            let Some(v) = inst.result.filter(|v| f.value_type(*v) == own) else {
                continue;
            };
            let fine = match &inst.kind {
                InstKind::Call(fid, _) if Some(fid.0 as usize) == alloc => stamped(v),
                InstKind::GlobalRef(name) => name == UNDEF_CELL_SYM,
                InstKind::IntToPtr(_) => false,
                _ => true,
            };
            if !fine {
                return false;
            }
        }
    }
    true
}
