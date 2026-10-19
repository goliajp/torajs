//! Which functions run, and how: reached by a direct call, entered
//! through an address, or never entered at all.

use crate::ssa::id_hash::HashSet;

use super::BOXED_ENTRY_PREFIX;
use crate::ssa::{FuncId, InstKind, Module};

/// Every function a direct call in a live function reaches, and every
/// one entered through an address (an instruction in a live function
/// takes it, or a table holds it).
pub(super) fn reached_and_taken(
    module: &Module,
    dead: &[bool],
) -> (HashSet<FuncId>, HashSet<FuncId>) {
    let mut reached = HashSet::default();
    let mut taken = table_held_fns(module);
    for (i, f) in module.funcs.iter().enumerate() {
        if dead[i] {
            continue;
        }
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            match &inst.kind {
                InstKind::Call(fid, _) => {
                    reached.insert(*fid);
                }
                InstKind::FnAddr(fid) | InstKind::BoxedEntryAddr(fid) => {
                    taken.insert(*fid);
                }
                _ => {}
            }
        }
    }
    (reached, taken)
}

/// Per function, whether it is a boxed adapter that no instruction
/// calls or takes the address of and no table holds. The runtime
/// enters an adapter only through an address stored for it, so such a
/// body never runs. Not transitive: an adapter referenced only from a
/// dead one counts as live.
pub(in crate::ssa_lower_pass_2_5) fn dead_adapters(module: &Module) -> Vec<bool> {
    let mut live = table_held_fns(module);
    for inst in module
        .funcs
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.insts)
    {
        if let InstKind::Call(fid, _) | InstKind::FnAddr(fid) | InstKind::BoxedEntryAddr(fid) =
            &inst.kind
        {
            live.insert(*fid);
        }
    }
    module
        .funcs
        .iter()
        .enumerate()
        .map(|(i, f)| f.name.starts_with(BOXED_ENTRY_PREFIX) && !live.contains(&FuncId(i as u32)))
        .collect()
}

/// Functions the runtime calls through a table: vtable slots, the
/// method rows' boxed adapters, the fn-name registry.
pub(super) fn table_held_fns(module: &Module) -> HashSet<FuncId> {
    let vtables = module
        .vtable_globals
        .iter()
        .flat_map(|v| v.fn_ids.iter().flatten().copied());
    let adapters = module.class_layouts.iter().flat_map(|c| {
        c.methods
            .iter()
            .flat_map(|m| std::iter::once(m.adapter_fid).chain(m.twin_adapter_fid))
    });
    let named = module.fn_name_globals.iter().map(|e| e.fn_id);
    let frames = (module.suspend_fns.iter()).flat_map(|s| [s.resume, s.drop, s.trace]);
    vtables.chain(adapters).chain(named).chain(frames).collect()
}
