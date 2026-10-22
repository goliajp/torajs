//! The pinned registers: inside a cluster (`Module::pins`) the field's
//! length lives in x27 and its data pointer in x28 for as long as a
//! member runs, so no member loads them.
//!
//! Within a cluster every member passes its own receiver on to the
//! others, and nothing that runs while one is on the stack changes an
//! array, so the two words a member would load are the ones its caller
//! saw. A member reads them as `InstKind::Pinned`. A member that
//! something else reaches (a call from outside the cluster or with
//! another receiver, an address taken, a table row) keeps its name as
//! an *entry*: it saves the caller's x27 / x28, loads the two words of
//! its receiver's field, calls the member's body under a local name,
//! and restores them on the way out; every call inside the cluster with
//! the caller's own receiver goes straight to the body. A body leaves
//! both registers as it found them, and every other function keeps
//! them as callee-saved, so a member finds them loaded wherever it is
//! entered from.
//!
//! The entry passes a throw on the way a lowered call does: it probes
//! the flag after the call, so a body that hands throws back in x21
//! keeps doing so.
//!
//! A cluster whose members never call each other with their receiver
//! gains nothing: every call would go through an entry, which loads
//! what the member would have, and pays its frame. It is left alone.
//!
//! Runs last, after every inline round and the block layout; codegen
//! reads `Module::pin_regs` (`compile/pin_reg.rs`).
//! `TORAJS_PIN_CLUSTER_OFF=1` skips, `TORAJS_PIN_CLUSTER_STATS=1` counts.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    FuncId, Function, InstKind, LOCAL_LABEL_PREFIX, Module, Operand, PinFact, PinWord, Type,
    ValueId,
};

use crate::load_licm::{ARR_DATA_PTR_OFF, ARR_LEN_OFF};
use rewrite::{entry, pin_words, retarget};

#[derive(Debug, Default)]
pub struct PinClusterStats {
    pub clusters: u32,
    pub members: u32,
    pub entries: u32,
    /// Loads read from the registers instead.
    pub loads: u32,
}

pub fn pin_clusters(module: &mut Module) -> PinClusterStats {
    let mut stats = PinClusterStats::default();
    let mut claimed: HashSet<FuncId> = HashSet::default();
    for fact in module.pins.clone() {
        let Some(members) = members(module, &fact, &claimed) else {
            continue;
        };
        if !calls_within(module, &members) {
            continue;
        }
        claimed.extend(members.iter().copied());
        place(module, &fact, &members, &mut stats);
    }
    stats
}

/// The members of `fact`'s cluster, when every one can take the
/// registers and some reads a word: a body with the class's receiver
/// first, in no other cluster, whose word loads feed no copy (a copy
/// may share its result's home, which a later def would overwrite).
fn members(module: &Module, fact: &PinFact, claimed: &HashSet<FuncId>) -> Option<Vec<FuncId>> {
    let mut reads = 0;
    for &m in &fact.cluster {
        let f = module.funcs.get(m.0 as usize)?;
        let this = *f.params.first()?;
        if f.is_declaration() || claimed.contains(&m) || f.value_type(this) != Type::Obj(fact.sid) {
            return None;
        }
        let words = words(f, fact.field_off);
        let copied = (f.blocks.iter().flat_map(|b| &b.insts)).any(|i| match &i.kind {
            InstKind::Copy(_, Operand::Value(v)) => words.contains_key(v),
            _ => false,
        });
        if copied {
            return None;
        }
        reads += words.len();
    }
    (reads > 0).then(|| fact.cluster.clone())
}

/// Whether some member calls a member with its own receiver.
fn calls_within(module: &Module, members: &[FuncId]) -> bool {
    members.iter().any(|m| {
        let f = &module.funcs[m.0 as usize];
        let alias = aliases(f);
        (f.blocks.iter().flat_map(|b| &b.insts)).any(|i| match &i.kind {
            InstKind::Call(t, args) => members.contains(t) && passes_own(f, &alias, args),
            _ => false,
        })
    })
}

/// Each load in `f` of its receiver's field's length or data pointer,
/// with the word it loads.
fn words(f: &Function, field_off: u64) -> HashMap<ValueId, PinWord> {
    let this = f.params[0];
    let alias = aliases(f);
    let root = |v: &ValueId| *alias.get(v).unwrap_or(v);
    let mut fields: HashSet<ValueId> = HashSet::default();
    let mut out = HashMap::default();
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        let (InstKind::Load(ty, Operand::Value(base), off), Some(r)) = (&inst.kind, inst.result)
        else {
            continue;
        };
        let base = root(base);
        if base == this && *off == field_off {
            fields.insert(r);
        } else if fields.contains(&base) && *off == ARR_LEN_OFF && *ty == Type::I64 {
            out.insert(r, PinWord::Len);
        } else if fields.contains(&base) && *off == ARR_DATA_PTR_OFF && *ty == Type::Ptr {
            out.insert(r, PinWord::Data);
        }
    }
    out
}

/// Each `id` result, and each result of a copy that is its value's only
/// def, resolved to the value at the end of its chain.
fn aliases(f: &Function) -> HashMap<ValueId, ValueId> {
    let mut defs: HashMap<ValueId, u32> = HashMap::default();
    let mut direct: HashMap<ValueId, ValueId> = HashMap::default();
    for inst in f.blocks.iter().flat_map(|b| &b.insts) {
        let Some(r) = inst.result else { continue };
        *defs.entry(r).or_default() += 1;
        if let InstKind::Identity(Operand::Value(x)) | InstKind::Copy(_, Operand::Value(x)) =
            &inst.kind
        {
            direct.insert(r, *x);
        }
    }
    direct.retain(|r, _| defs[r] == 1);
    let mut out = HashMap::default();
    for &r in direct.keys() {
        let mut v = r;
        let mut steps = 0;
        while let Some(x) = direct.get(&v) {
            v = *x;
            steps += 1;
            if steps > direct.len() {
                break;
            }
        }
        out.insert(r, v);
    }
    out
}

/// Whether a call with `args` in `f` passes `f`'s own receiver.
fn passes_own(f: &Function, alias: &HashMap<ValueId, ValueId>, args: &[Operand]) -> bool {
    match args.first() {
        Some(Operand::Value(a)) => *alias.get(a).unwrap_or(a) == f.params[0],
        _ => false,
    }
}

/// Give `members` the registers: entries for those something else
/// reaches, the loads of the two words read from the registers.
fn place(module: &mut Module, fact: &PinFact, members: &[FuncId], stats: &mut PinClusterStats) {
    let set: HashSet<FuncId> = members.iter().copied().collect();
    let quiet: HashSet<FuncId> = fact.quiet.iter().copied().collect();
    let reached = reached_from_outside(module, &set, &quiet);
    let mut body_of: HashMap<FuncId, FuncId> = HashMap::default();
    let mut names: HashSet<String> = module.funcs.iter().map(|f| f.name.clone()).collect();
    for &m in members.iter().filter(|m| reached.contains(m)) {
        let mut name = format!(
            "{LOCAL_LABEL_PREFIX}{}.pin",
            module.funcs[m.0 as usize].name
        );
        while names.contains(&name) {
            name.push('_');
        }
        names.insert(name.clone());
        let body_id = FuncId(module.funcs.len() as u32);
        let mut body = module.funcs[m.0 as usize].clone();
        body.name = name;
        module.funcs.push(body);
        module.funcs[m.0 as usize] = entry(&module.funcs[m.0 as usize], body_id);
        body_of.insert(m, body_id);
        module.pin_regs.entries.push((m, body_id, fact.field_off));
        stats.entries += 1;
    }
    for &m in members {
        let at = body_of.get(&m).copied().unwrap_or(m);
        let f = &mut module.funcs[at.0 as usize];
        stats.loads += pin_words(f, fact.field_off);
        retarget(f, &set, &body_of);
        module.pin_regs.inner.push(at);
        stats.members += 1;
    }
    stats.clusters += 1;
}

/// The members something reaches other than a call from a member with
/// that member's own receiver; what a function that never runs
/// (`quiet`) names reaches nothing.
fn reached_from_outside(
    module: &Module,
    set: &HashSet<FuncId>,
    quiet: &HashSet<FuncId>,
) -> HashSet<FuncId> {
    let mut out: HashSet<FuncId> = HashSet::default();
    for (i, f) in module.funcs.iter().enumerate() {
        if quiet.contains(&FuncId(i as u32)) {
            continue;
        }
        let inside = set.contains(&FuncId(i as u32));
        let alias = if inside {
            aliases(f)
        } else {
            HashMap::default()
        };
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            match &inst.kind {
                InstKind::Call(t, args) if set.contains(t) => {
                    if !(inside && passes_own(f, &alias, args)) {
                        out.insert(*t);
                    }
                }
                InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) if set.contains(t) => {
                    out.insert(*t);
                }
                _ => {}
            }
        }
    }
    let tables = (module.vtable_globals.iter())
        .flat_map(|v| v.fn_ids.iter().flatten().copied())
        .chain(module.fn_name_globals.iter().map(|e| e.fn_id))
        .chain(module.class_drop_fns.iter().copied())
        .chain((module.suspend_fns.iter()).flat_map(|s| [s.wrapper, s.resume, s.drop]))
        .chain(
            (module.class_layouts.iter().flat_map(|c| &c.methods))
                .flat_map(|m| std::iter::once(m.adapter_fid).chain(m.twin_adapter_fid)),
        );
    out.extend(tables.filter(|t| set.contains(t)));
    for &m in set {
        let name = &module.funcs[m.0 as usize].name;
        if name == "main" || name.starts_with("__torajs_") {
            out.insert(m);
        }
    }
    out
}

mod rewrite;
#[cfg(test)]
mod tests;
