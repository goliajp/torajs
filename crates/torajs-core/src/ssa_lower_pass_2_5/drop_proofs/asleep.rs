//! Which functions run only once some class instance or class object
//! reaches the runtime as a JS value.
//!
//! A class's method rows (`ClassLayoutMeta::methods`) hold the boxed
//! adapters the runtime enters for a method call on an `any` receiver
//! of that class, and for a foreign receiver handed a method value
//! (the `__cmany_` twin's adapter). The runtime has such a receiver or
//! method value only once an instance, a prototype or the class itself
//! became a JS value; until then nothing enters a row adapter, nor
//! anything only those adapters reach. Every other function counts as
//! running: one some instruction of a running function calls or takes
//! the address of, one a vtable or an async frame holds, a release fn,
//! and every function nothing references at all (the entry points).
//! The fn-name table is no entry: the runtime only looks names up in
//! it by address.

use crate::ssa::{FuncId, InstKind, Module};
use crate::ssa_lower_emit_drop_value::class_drop_fill::DropHooks;

const ADAPTER_PREFIX: &str = "__boxed_";

/// What a function names: its direct calls and the addresses it takes.
fn named(module: &Module, i: usize) -> impl Iterator<Item = FuncId> + '_ {
    let insts = module.funcs[i].blocks.iter().flat_map(|b| &b.insts);
    insts.filter_map(|inst| match &inst.kind {
        InstKind::Call(t, _) | InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) => Some(*t),
        _ => None,
    })
}

/// Per function, whether it runs only through a class method row,
/// where the rows `woken` marks (by index) are entered.
pub(super) fn row_only(module: &Module, woken: &[bool]) -> Vec<bool> {
    let n = module.funcs.len();
    let mut referenced = vec![false; n];
    for i in 0..n {
        named(module, i).for_each(|t| referenced[t.0 as usize] = true);
    }
    let mut row_held = vec![false; n];
    let mut entered = Vec::new();
    for (row, c) in module.class_layouts.iter().enumerate() {
        let on = woken.get(row).copied().unwrap_or(false);
        for m in &c.methods {
            for a in std::iter::once(m.adapter_fid).chain(m.twin_adapter_fid) {
                if on {
                    entered.push(a);
                } else {
                    row_held[a.0 as usize] = true;
                }
            }
        }
    }
    let entry = |i: usize| {
        let f = &module.funcs[i];
        !referenced[i] && !row_held[i] && !f.name.starts_with(ADAPTER_PREFIX)
    };
    let mut stack: Vec<FuncId> = (0..n)
        .filter(|i| entry(*i))
        .map(|i| FuncId(i as u32))
        .collect();
    let vtables = (module.vtable_globals.iter()).flat_map(|v| v.fn_ids.iter().flatten().copied());
    let frames = (module.suspend_fns.iter()).flat_map(|s| [s.resume, s.drop, s.trace]);
    stack.extend(
        vtables
            .chain(frames)
            .chain(module.class_drop_fns.iter().copied())
            .chain(entered),
    );
    let mut runs = vec![false; n];
    while let Some(fid) = stack.pop() {
        let i = fid.0 as usize;
        if std::mem::replace(&mut runs[i], true) {
            continue;
        }
        stack.extend(named(module, i).filter(|t| !runs[t.0 as usize]));
    }
    (module.funcs.iter().zip(runs))
        .map(|(f, r)| !r && !f.blocks.is_empty())
        .collect()
}

/// The functions only a class method row enters once the rows of the
/// class sids `woken` names are counted as entered too.
pub(crate) fn woken(module: &Module, hooks: &DropHooks, woken: &[bool]) -> Vec<bool> {
    let mut rows = vec![false; module.class_layouts.len()];
    for (sid, tags) in &hooks.tags {
        if woken.get(*sid as usize).copied().unwrap_or(false) {
            // a row is indexed by its tag less one; tag 0 is no class
            for t in tags.iter().filter(|t| **t > 0) {
                rows[*t as usize - 1] = true;
            }
        }
    }
    row_only(module, &rows)
}
