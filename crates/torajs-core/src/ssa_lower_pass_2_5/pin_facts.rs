//! Which array fields of a class hold still while a group of its
//! methods runs on one receiver (`Module::pins`).
//!
//! A read `this.f[i]` loads the field, then the array's length and its
//! data pointer. Nothing in the SSA says a call leaves those three
//! words alone, so every read after a call loads them again. For a
//! field `f` of class sid `s` that holds an array, they stay as they
//! are while a *cluster* of `s`'s methods runs when
//!
//! - F1, only construction writes `f`: `s` is sealed and closed
//!   (`drop_proofs`) and no slot of another layout reads its cells, `f`
//!   is never read nullish (`live_field_census`), and every store into
//!   `f` by a function that runs writes a cell that function allocated
//!   itself, or is the constructor's on its receiver while every call
//!   of the constructor passes a cell its caller allocated;
//! - F2, nothing that can run while a member is on the stack changes
//!   an array ([`grow`]);
//! - F3, each call from one member to another passes the caller's own
//!   receiver; a call between two candidates that passes another cell
//!   puts both out.
//!
//! F4, that nothing but a direct call enters a member, is recorded
//! (`PinFact::direct_only`), not required. Each cluster also records
//! the words it could keep in a register while it runs ([`scalars`]).
//! Only facts are written; no instruction changes. `TORAJS_PIN_STATS=1`
//! prints each field's answer.

mod cluster;
mod grow;
mod names;
mod roots;
mod scalars;
#[cfg(test)]
mod tests;
pub(crate) mod woken;

use std::collections::{HashMap, HashSet};

use crate::ssa::{ArrId, FuncId, Function, InstKind, Module, Operand, StructId, Type, ValueId};
use crate::ssa_lower::OBJ_HEADER_SIZE;
use crate::ssa_lower::proofs::ProgramFacts;
use crate::ssa_lower_emit_drop_value::class_drop_fill::DropHooks;

use super::drop_proofs::Seal;
use cluster::Clusters;
pub(crate) use names::sole_names;
use roots::{Root, RootsByFn};

const OBJ_ALLOC: &str = "__torajs_obj_alloc";

fn stats() -> bool {
    std::env::var_os("TORAJS_PIN_STATS").is_some()
}

/// A store into a cell of a watched sid: at a field offset, or, for an
/// element held in place or a computed offset, at any (`None`).
struct Write {
    fid: FuncId,
    sid: StructId,
    off: Option<u64>,
    base: ValueId,
}

/// What `cell_slot_flow`'s scan of the functions that run records for
/// the census, in the same walk.
pub(crate) struct Watch {
    /// Per sid, the index and layout of each array field; empty for a
    /// sid that is not a class or holds no array.
    fields: Vec<Vec<(usize, ArrId)>>,
    /// Per function, whether its first parameter is a cell of a
    /// watched sid.
    methods: Vec<bool>,
    writes: Vec<Write>,
    /// Each direct call of such a function, with its caller.
    calls: Vec<(FuncId, FuncId)>,
    /// Such functions whose address is taken.
    taken: HashSet<FuncId>,
}

impl Watch {
    /// `None` when no class holds an array: the census has nothing to
    /// ask, and the scan records nothing.
    pub(crate) fn of(module: &Module, class_sids: &HashSet<u32>) -> Option<Watch> {
        let mut fields = vec![Vec::new(); module.struct_layouts.len()];
        for &sid in class_sids {
            let layout = &module.struct_layouts[sid as usize];
            for (i, (_, ty)) in layout.iter().enumerate() {
                if let Type::Arr(a) = ty {
                    fields[sid as usize].push((i, *a));
                }
            }
        }
        if fields.iter().all(Vec::is_empty) {
            return None;
        }
        let watched = |f: &Function| {
            f.params.first().is_some_and(
                |p| matches!(f.value_type(*p), Type::Obj(s) if !fields[s.0 as usize].is_empty()),
            )
        };
        let methods = module.funcs.iter().map(watched).collect();
        Some(Watch {
            fields,
            methods,
            writes: Vec::new(),
            calls: Vec::new(),
            taken: HashSet::new(),
        })
    }

    /// The sids holding an array field.
    pub(crate) fn sids(&self) -> Vec<u32> {
        (0..self.fields.len() as u32)
            .filter(|s| !self.fields[*s as usize].is_empty())
            .collect()
    }

    pub(crate) fn note(&mut self, fid: FuncId, f: &Function, kind: &InstKind) {
        let cell = |op: &Operand| match op {
            Operand::Value(b) => match f.value_type(*b) {
                Type::Obj(s) if !self.fields[s.0 as usize].is_empty() => Some((s, *b, true)),
                Type::Inline(s) if !self.fields[s.0 as usize].is_empty() => Some((s, *b, false)),
                _ => None,
            },
            _ => None,
        };
        let write = match kind {
            InstKind::Store(_, p, off) => {
                cell(p).map(|(sid, base, obj)| (sid, base, obj.then_some(*off)))
            }
            InstKind::StoreDyn(_, p, _) | InstKind::StoreDynScaled8(_, p, _) => {
                cell(p).map(|(sid, base, _)| (sid, base, None))
            }
            InstKind::Call(t, _) if self.methods[t.0 as usize] => {
                self.calls.push((fid, *t));
                None
            }
            InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) if self.methods[t.0 as usize] => {
                self.taken.insert(*t);
                None
            }
            _ => None,
        };
        if let Some((sid, base, off)) = write {
            self.writes.push(Write {
                fid,
                sid,
                off,
                base,
            });
        }
    }
}

/// Write `module.pins` from what `watch` saw; `seals` answers for the
/// watched sids (`drop_proofs::settle`), `skip` names the functions
/// that never run. Runs after the release fns' tests are settled: a
/// store base `watch` holds still names the value it did, or one no
/// instruction defines any longer, which reads as no root.
pub(crate) fn settle(
    module: &mut Module,
    watch: Watch,
    seals: &HashMap<u32, Seal>,
    hooks: &DropHooks,
    facts: &ProgramFacts,
    skip: &[bool],
) {
    let alloc = (module.funcs.iter().position(|f| f.name == OBJ_ALLOC)).map(|i| FuncId(i as u32));
    let mut pins = Vec::new();
    let mut clusters = Clusters::new(module, &watch, skip, alloc);
    for sid in watch.sids() {
        let class = hooks.names.get(&sid).map(String::as_str);
        for &(idx, arr) in &watch.fields[sid as usize] {
            let off = OBJ_HEADER_SIZE + 8 * idx as u64;
            let name = module.struct_layouts[sid as usize][idx]
                .0
                .to_string_lossy_owned();
            let label = format!("sid {sid} {}.{name}", class.unwrap_or("?"));
            let field = Field {
                sid,
                off,
                name: &name,
            };
            let only_ctor = match class {
                Some(c) => sealed(seals, sid)
                    .and_then(|_| only_ctor(module, &watch, facts, alloc, &field, c)),
                None => Err("no class name of its own".to_string()),
            };
            if let Err(why) = only_ctor {
                if stats() {
                    eprintln!("[pin] {label}: none: {why}");
                }
                continue;
            }
            for fact in clusters.pins(StructId(sid), off, arr, &label) {
                pins.push(fact);
            }
        }
    }
    if stats() {
        eprintln!("[pin] {} pins", pins.len());
    }
    module.pins = pins;
}

/// An array field of a watched sid, by its offset in the cell and its
/// own name: a name may hold a dot, so it is never read back out of a
/// printed label.
struct Field<'a> {
    sid: u32,
    off: u64,
    name: &'a str,
}

/// The seal F1 asks of class sid `sid`.
fn sealed(seals: &HashMap<u32, Seal>, sid: u32) -> Result<(), String> {
    let seal = seals.get(&sid).copied().ok_or("no verdict")?;
    if !(seal.sealed && seal.closed && seal.unwidened) {
        return Err(format!(
            "sealed {} closed {} unwidened {}",
            seal.sealed, seal.closed, seal.unwidened
        ));
    }
    Ok(())
}

/// F1 for `field`, a field of the class named `class`, but the seal.
fn only_ctor(
    module: &Module,
    watch: &Watch,
    facts: &ProgramFacts,
    alloc: Option<FuncId>,
    field: &Field,
    class: &str,
) -> Result<(), String> {
    if !facts
        .live
        .fields
        .contains(&(class.to_string(), field.name.to_string()))
    {
        return Err("may be read nullish".into());
    }
    let ctor = format!("__cm_{class}__ctor");
    let ctor = (module.funcs.iter().position(|f| f.name == ctor)).map(|i| FuncId(i as u32));
    let mut roots = RootsByFn::new(module, alloc);
    let mut ctor_writes = false;
    let writes = (watch.writes.iter())
        .filter(|w| w.sid.0 == field.sid && w.off.is_none_or(|o| o == field.off));
    for w in writes {
        match roots.root(w.fid, &Operand::Value(w.base)) {
            Root::Fresh => {}
            Root::Receiver if Some(w.fid) == ctor => ctor_writes = true,
            _ => {
                return Err(format!(
                    "written by {}",
                    module.funcs[w.fid.0 as usize].name
                ));
            }
        }
    }
    match ctor.filter(|_| ctor_writes) {
        Some(ctor) => ctor_on_fresh_cells(module, watch, &mut roots, ctor),
        None => Ok(()),
    }
}

/// Nothing takes the address of the constructor `ctor`, and every call
/// of it passes a cell its caller allocated.
fn ctor_on_fresh_cells(
    module: &Module,
    watch: &Watch,
    roots: &mut RootsByFn,
    ctor: FuncId,
) -> Result<(), String> {
    if watch.taken.contains(&ctor) {
        return Err("the constructor's address is taken".into());
    }
    for &(caller, _) in watch.calls.iter().filter(|(_, t)| *t == ctor) {
        let f = &module.funcs[caller.0 as usize];
        let fresh = (f.blocks.iter().flat_map(|b| &b.insts)).all(|inst| match &inst.kind {
            InstKind::Call(t, args) if *t == ctor => {
                (args.first()).is_some_and(|a| roots.root(caller, a) == Root::Fresh)
            }
            _ => true,
        });
        if !fresh {
            return Err(format!(
                "the constructor runs on a cell {} did not allocate",
                f.name
            ));
        }
    }
    Ok(())
}
