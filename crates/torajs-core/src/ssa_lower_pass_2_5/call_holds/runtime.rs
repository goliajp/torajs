//! The user code a runtime call may run, for the runtime reads that run
//! it only through the language's own hooks.

use crate::ssa::{FuncId, InstKind, Module};

/// Runtime reads that may run user code through a property's getter or
/// a value's conversion hook (`valueOf`, `toString`), and release only
/// the values they made themselves: a typed array's read at a key that
/// is not an index (§7.1.19, the property "1.5"), which looks the key
/// up on the array's own properties and its prototype chain. What they
/// release was never anyone else's, so they free nothing the caller
/// can still reach; what the user code they run releases is what the
/// functions the runtime can enter release.
pub(super) const REENTRANT_READS: &[&str] = &[
    "__torajs_arr_typed_nonint_f64",
    "__torajs_arr_typed_nonint_str",
    "__torajs_arr_typed_nonint_any",
];

/// Runtime calls that allocate a cell or grow an array's storage: they
/// run no user code and release nothing.
pub(super) const ALLOCS: &[&str] = &["__torajs_arr_alloc", "__torajs_arr_reserve"];

/// The array releases. They run no user code (a release frees cells and
/// queues finalization callbacks, never runs one); what they may free
/// is judged by the type of the array they are handed.
pub(super) const ARR_DROP_PREFIX: &str = "__torajs_arr_drop";

/// The functions the runtime can enter: those a table holds (vtable
/// slots, method rows, a frame's entries) and those whose address an
/// instruction takes, but the ones `asleep` marks, which only a class
/// method row nobody wakes enters, and the instructions in them. The
/// fn-name registry is no entry: the runtime only looks a name up in
/// it by address.
pub(super) fn entered(module: &Module, asleep: &[bool]) -> Vec<FuncId> {
    let awake = |f: &FuncId| !asleep.get(f.0 as usize).copied().unwrap_or(false);
    let vtables = (module.vtable_globals.iter()).flat_map(|v| v.fn_ids.iter().flatten().copied());
    let rows = (module.class_layouts.iter()).flat_map(|c| {
        (c.methods.iter()).flat_map(|m| std::iter::once(m.adapter_fid).chain(m.twin_adapter_fid))
    });
    let frames = (module.suspend_fns.iter()).flat_map(|s| [s.resume, s.drop, s.trace]);
    let mut out: Vec<FuncId> = vtables.chain(rows).chain(frames).filter(awake).collect();
    for (i, f) in module.funcs.iter().enumerate() {
        if !awake(&FuncId(i as u32)) {
            continue;
        }
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            if let InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) = &inst.kind
                && awake(t)
            {
                out.push(*t);
            }
        }
    }
    out.sort_by_key(|f| f.0);
    out.dedup();
    out
}
