//! SD-4c-prereq+e3 — `__torajs_fn_<fid>` sym → vaddr registration.
//!
//! Codegen's `emit_fn_addr` (taking a function pointer at the SSA
//! layer) emits an ADRP+ADD pair targeting `__torajs_fn_<fid>`. The
//! ID is the SSA `FuncId(n)` value, which equals the user-fn index
//! into `LinkConfig.funcs` (codegen compiles them in vec order).
//!
//! `apply_relocs::lookup_or_defined` already resolves user-fn names
//! (`f.name == "<entry>"`) via `fn_vaddrs[i]`, but the codegen-emitted
//! `__torajs_fn_<i>` sym is *not* a user-fn `.name` — it's a parallel
//! naming scheme. This module injects one alias per user fn into the
//! effective sym table so the ADRP+ADD pair resolves to `fn_vaddrs[i]`.

use crate::resolve::SymTable;
use std::collections::BTreeSet;
use torajs_codegen::CompiledFunction;

/// Sym names to flag as link-defined in the worklist closure
/// (`compute_required_members::extra_defined_syms`) so a user-fn
/// reloc against `__torajs_fn_<i>` isn't surfaced as `UnresolvedExterns`
/// before the emit pass injects the alias.
pub fn fn_addr_extra_defined_syms(funcs: &[CompiledFunction]) -> BTreeSet<String> {
    (0..funcs.len())
        .map(|i| format!("__torajs_fn_{i}"))
        .collect()
}

/// Register `__torajs_fn_<i>` → `fn_vaddrs[i]` for every user fn.
/// `funcs.len() == fn_vaddrs.len()` (the layout pass invariant); this
/// helper trusts that and panics in debug otherwise.
pub fn register_fn_addr_syms(
    funcs: &[CompiledFunction],
    fn_vaddrs: &[u64],
    sym_table: &mut SymTable,
) {
    debug_assert_eq!(
        funcs.len(),
        fn_vaddrs.len(),
        "funcs and fn_vaddrs must zip 1:1"
    );
    for (i, vaddr) in fn_vaddrs.iter().enumerate() {
        sym_table.insert(format!("__torajs_fn_{i}"), *vaddr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::frame::FrameLayout;

    fn dummy_fn(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: Vec::new(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    #[test]
    fn empty_input_no_inserts() {
        let mut sym_table = SymTable::new();
        register_fn_addr_syms(&[], &[], &mut sym_table);
        assert!(sym_table.is_empty());
    }

    #[test]
    fn one_entry_per_fn_id() {
        let funcs = [dummy_fn("_main"), dummy_fn("_helper")];
        let fn_vaddrs = [0x1_0000_4000u64, 0x1_0000_4020u64];
        let mut sym_table = SymTable::new();
        register_fn_addr_syms(&funcs, &fn_vaddrs, &mut sym_table);
        assert_eq!(sym_table.get("__torajs_fn_0"), Some(&0x1_0000_4000));
        assert_eq!(sym_table.get("__torajs_fn_1"), Some(&0x1_0000_4020));
    }

    #[test]
    fn overrides_existing_entry() {
        // An entry already in sym_table for `__torajs_fn_0` (caller
        // hand-filled) gets overwritten — the link-pass-computed vaddr
        // is the source of truth.
        let funcs = [dummy_fn("_main")];
        let fn_vaddrs = [0x1_0000_4000u64];
        let mut sym_table = SymTable::new();
        sym_table.insert("__torajs_fn_0".into(), 0xDEAD_BEEF);
        register_fn_addr_syms(&funcs, &fn_vaddrs, &mut sym_table);
        assert_eq!(sym_table.get("__torajs_fn_0"), Some(&0x1_0000_4000));
    }
}
