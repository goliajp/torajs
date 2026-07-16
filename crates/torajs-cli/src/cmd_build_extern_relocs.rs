//! Extract of `rewrite_extern_relocs` + `parse_fn_addr_sym` from
//! [`crate::cmd_build`] (file-size-debt trim). Rewrites `Func(fid)`
//! call-site relocs whose target is an SSA extern declaration into
//! `Extern("_<name>")` so the link layer resolves them through the
//! archive symbol table (`___torajs_*` Apple form) instead of a
//! stale fn_vaddrs slot. Page21 / PageOff12 / AbsPtr64 relocs whose
//! `target_sym = "__torajs_fn_<fid>"` point at an extern get the
//! same treatment — FnAddr of an extern becomes an external sym ref.

use torajs_codegen::reloc::{CallTarget, RelocKind};

/// Rewrite `CallSite{Func(fid)}` relocs that target an extern declaration
/// into `CallSite{Extern("_<name>")}` so the link layer resolves them
/// through the archive symbol table (`___torajs_*` Apple form) instead
/// of a stale fn_vaddrs slot. Page21 / PageOff12 / AbsPtr64 with
/// `target_sym = "__torajs_fn_<fid>"` pointing at an extern get the same
/// treatment — FnAddr of an extern becomes an external sym ref.
pub(crate) fn rewrite_extern_relocs(
    compiled: &mut [torajs_codegen::CompiledFunction],
    ssa_funcs: &[torajs_core::ssa::Function],
) {
    let is_extern: Vec<bool> = ssa_funcs.iter().map(|f| f.is_declaration()).collect();
    for cf in compiled.iter_mut() {
        for r in cf.relocs.iter_mut() {
            match &mut r.kind {
                RelocKind::CallSite {
                    target: CallTarget::Func(fid),
                } if is_extern[fid.0 as usize] => {
                    let name = ssa_funcs[fid.0 as usize].name.clone();
                    r.kind = RelocKind::CallSite {
                        target: CallTarget::Extern(format!("_{name}")),
                    };
                }
                RelocKind::Page21 { target_sym }
                | RelocKind::PageOff12 { target_sym }
                | RelocKind::AbsPtr64 { target_sym } => {
                    if let Some(fid) = parse_fn_addr_sym(target_sym)
                        && fid < is_extern.len()
                        && is_extern[fid]
                    {
                        *target_sym = format!("_{}", ssa_funcs[fid].name);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse `"__torajs_fn_<n>"` → `<n>` (the original FuncId index), or
/// `None` for any other sym name. Matches the codegen FnAddr convention
/// in `crates/torajs-codegen/src/compile/refs.rs:106`.
fn parse_fn_addr_sym(sym: &str) -> Option<usize> {
    sym.strip_prefix("__torajs_fn_")
        .and_then(|tail| tail.parse().ok())
}
