//! The census when an instance of some class reaches the runtime.
//!
//! The release fns' tests are then left as they are (`drop_proofs`):
//! the runtime can enter that class's method rows, which the scans
//! counted as asleep. The census only needs what holds of the classes
//! it asks about, so it reads the module again with those rows
//! entered: the rows of each class whose instances reach the runtime,
//! and of every class whose layout is a prefix of such a class's (its
//! base classes among them, whose prototypes the runtime reaches from
//! the instance). What the newly entered functions expose wakes more
//! rows, until nothing new is exposed. A class the runtime holds an
//! instance of is never sealed, so it is never pinned. The scans are
//! read again only when what the first one saw leaves some field that
//! could be pinned (`may_pin`): the functions the rows enter only add
//! stores and calls.

use std::collections::HashSet;

use crate::ssa::{FuncId, Module, StructId};
use crate::ssa_lower::OBJ_HEADER_SIZE;
use crate::ssa_lower::proofs::ProgramFacts;
use crate::ssa_lower_emit_drop_value::class_drop_fill::DropHooks;

use super::super::cell_release::{Slots, wider_cells};
use super::super::cell_slot_flow::{Both, EnvLayouts, Scanned, is_prefix, open_sids_more};
use super::super::drop_proofs;
use super::cluster::Clusters;
use super::{Field, Watch, only_ctor, stats};

/// Write `module.pins` reading the module with the rows of the
/// classes in `exposed` (and those they expose in turn) entered;
/// `first` is what the scan with the rows asleep showed the census and
/// added up to, and the functions it ruled out. Each round scans only
/// the functions the newly entered rows add.
pub(crate) fn settle(
    module: &mut Module,
    envs: &EnvLayouts,
    hooks: &DropHooks,
    facts: &ProgramFacts,
    first: (Watch, Scanned, &[bool]),
    mut exposed: Vec<u32>,
) {
    let (mut watch, mut scanned, before) = first;
    if !may_pin(module, &watch, hooks, facts, before) {
        if stats() {
            eprintln!("[pin] 0 pins: no field could be pinned");
        }
        return;
    }
    let mut woken = vec![false; module.struct_layouts.len()];
    let mut before = before.to_vec();
    loop {
        wake(module, &hooks.class_sids, &mut woken, &exposed);
        if stats() {
            let w: Vec<String> = (0..woken.len())
                .filter(|s| woken[*s])
                .map(|s| s.to_string())
                .collect();
            eprintln!("[pin] the rows of class sids {{{}}} entered", w.join(" "));
        }
        let asleep = drop_proofs::asleep_woken(module, hooks, &woken);
        let Both {
            open,
            widens,
            skip,
            pins,
            scanned: more,
            ..
        } = open_sids_more(module, envs, (scanned, &before), &asleep, watch);
        let (Some(w), Some(more)) = (pins, more) else {
            return;
        };
        let wider = wider_cells(module.struct_layouts.len(), &widens);
        let flow = (open.clone(), widens, skip.clone());
        match drop_proofs::seals(module, envs, hooks, flow, &w.sids(), &woken) {
            Ok(seals) => {
                let slots = Slots::new(&open, &wider);
                return super::settle(module, w, &seals, hooks, facts, (&skip, slots));
            }
            Err(again) => (exposed, watch, scanned, before) = (again, w, more, skip),
        }
    }
}

/// Mark in `woken` the class sids of `exposed` and every class sid
/// whose layout is a prefix of one of theirs.
fn wake(module: &Module, classes: &HashSet<u32>, woken: &mut [bool], exposed: &[u32]) {
    let layouts = &module.struct_layouts;
    for &x in exposed {
        woken[x as usize] = true;
        for &c in classes {
            if is_prefix(&layouts[c as usize], &layouts[x as usize]) {
                woken[c as usize] = true;
            }
        }
    }
}

/// Whether some array field `watch` saw could be pinned once the
/// seals are known: F1 but the seal holds of it, and some cluster reads
/// it and changes no array while it runs. Over the functions that run
/// besides those `skip` names, a pin never holds where this does not.
fn may_pin(
    module: &Module,
    watch: &Watch,
    hooks: &DropHooks,
    facts: &ProgramFacts,
    skip: &[bool],
) -> bool {
    let alloc =
        (module.funcs.iter().position(|f| f.name == super::OBJ_ALLOC)).map(|i| FuncId(i as u32));
    let mut clusters = Clusters::new(module, watch, skip, alloc);
    watch.sids().into_iter().any(|sid| {
        let Some(class) = hooks.names.get(&sid) else {
            return false;
        };
        watch.fields[sid as usize].iter().any(|&(idx, _)| {
            let off = OBJ_HEADER_SIZE + 8 * idx as u64;
            let name = module.struct_layouts[sid as usize][idx]
                .0
                .to_string_lossy_owned();
            let field = Field {
                sid,
                off,
                name: &name,
            };
            only_ctor(module, watch, facts, alloc, &field, class).is_ok()
                && clusters.any(StructId(sid), off)
        })
    })
}
