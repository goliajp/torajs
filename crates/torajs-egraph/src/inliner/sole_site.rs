//! Callees whose only use is one direct call — LLVM's last-call-to-
//! static bonus. Splicing such a body moves it rather than copying it
//! (nothing else can reach the out-of-line copy, so dead stripping
//! drops it), so the per-callee cost ceiling, which exists to bound
//! code growth, does not apply; a far larger ceiling still bounds
//! the caller's compile time and register pressure.

use torajs_core::ssa::{InstKind, Module};

/// `sole[i]`: `module.funcs[i]` is called from exactly one direct call
/// site, never by itself, and its address is never taken (a
/// `FnAddr` / `BoxedEntryAddr` reference can call it from anywhere).
pub(super) fn sole_site_callees(module: &Module) -> Vec<bool> {
    let n = module.funcs.len();
    let mut calls = vec![0u32; n];
    let mut escapes = vec![false; n];
    for (caller, func) in module.funcs.iter().enumerate() {
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            match &inst.kind {
                InstKind::Call(id, _) => {
                    let i = id.0 as usize;
                    calls[i] += 1;
                    escapes[i] |= i == caller;
                }
                InstKind::FnAddr(id) | InstKind::BoxedEntryAddr(id) => {
                    escapes[id.0 as usize] = true;
                }
                _ => {}
            }
        }
    }
    (0..n).map(|i| calls[i] == 1 && !escapes[i]).collect()
}
