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

const ADAPTER_PREFIX: &str = "__boxed_";

/// Per function, whether it runs only through a class method row.
pub(super) fn row_only(module: &Module) -> Vec<bool> {
    let n = module.funcs.len();
    let mut edges: Vec<Vec<FuncId>> = vec![Vec::new(); n];
    let mut referenced = vec![false; n];
    for (i, f) in module.funcs.iter().enumerate() {
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            if let InstKind::Call(t, _) | InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) =
                &inst.kind
            {
                edges[i].push(*t);
                referenced[t.0 as usize] = true;
            }
        }
    }
    let mut row_held = vec![false; n];
    for m in module.class_layouts.iter().flat_map(|c| &c.methods) {
        row_held[m.adapter_fid.0 as usize] = true;
        if let Some(t) = m.twin_adapter_fid {
            row_held[t.0 as usize] = true;
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
            .chain(module.class_drop_fns.iter().copied()),
    );
    let mut runs = vec![false; n];
    while let Some(fid) = stack.pop() {
        let i = fid.0 as usize;
        if std::mem::replace(&mut runs[i], true) {
            continue;
        }
        stack.extend(edges[i].iter().filter(|t| !runs[t.0 as usize]));
    }
    (module.funcs.iter().zip(runs))
        .map(|(f, r)| !r && !f.blocks.is_empty())
        .collect()
}
