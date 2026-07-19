//! SSA → link-layer registry bridges built once per `tr build` /
//! `tr run`. Carved out of `cmd_build.rs::build_link_config` in
//! W-J Phase A3c chunk 2 to keep `cmd_build.rs` below its file-size
//! debt ceiling after the class-name registry intern step landed.
//!
//! - `build_user_strings`: `ssa::Module.strings` → both
//!   StaticStr (`__torajs_str_lit_<i>`) + RawBytes
//!   (`__torajs_str_dyn_<i>`) flavour entries.
//! - `build_class_names`: `ssa::Module.class_layouts` →
//!   `(UserClassNameEntry, UserStringEntry)` pairs for each named
//!   class, appended to the strings vec under
//!   `__torajs_class_name_str_<tag>`.
//! - `build_fn_name_globals`: `ssa::Module.fn_name_globals` →
//!   `UserFnNameEntry` Vec keyed on existing `__torajs_str_dyn_<sid>`
//!   alias from the strings table.

use torajs_core::ssa::Module;
use torajs_link::exec::{UserClassNameEntry, UserFnNameEntry, UserStringEntry, UserStringKind};

pub fn build_user_strings(ssa_module: &Module) -> Vec<UserStringEntry> {
    let mut strings: Vec<UserStringEntry> = Vec::with_capacity(ssa_module.strings.len() * 2);
    for (i, lit) in ssa_module.strings.iter().enumerate() {
        strings.push(UserStringEntry {
            sym: format!("__torajs_str_lit_{i}"),
            bytes: lit.bytes.clone(),
            is_latin1: lit.is_latin1,
            length: lit.length,
            kind: UserStringKind::StaticStr,
        });
        strings.push(UserStringEntry {
            sym: format!("__torajs_str_dyn_{i}"),
            bytes: lit.bytes.clone(),
            is_latin1: lit.is_latin1,
            length: lit.length,
            kind: UserStringKind::RawBytes,
        });
    }
    strings
}

/// Append a RawBytes string for each named class's source-text name
/// to `strings` and return the matching `class_names` Vec. class_tag
/// = `i + 1` mirrors the ssa 1-based convention (0 reserved for
/// anonymous / non-stamped). Sym pattern `__torajs_class_name_str_<tag>`
/// is link-layer-internal — only `apply_user_string_overrides`
/// (sym-driven registration) consumes it.
pub fn build_class_names(
    ssa_module: &Module,
    strings: &mut Vec<UserStringEntry>,
) -> Vec<UserClassNameEntry> {
    let mut class_names: Vec<UserClassNameEntry> = Vec::new();
    for (i, meta) in ssa_module.class_layouts.iter().enumerate() {
        if meta.class_name.is_empty() {
            continue;
        }
        // W-J Phase A1 follow-up — anonymous ObjectLit class_layouts
        // entries use the link-layer-internal sym pattern
        // `__anon_struct_<sid_idx>` (ssa_lower::lower_module's anon
        // snapshot + fresh-sid loops). Skip them here so the runtime
        // `__torajs_struct_class_name(tag)` returns NULL for anon tags
        // → `struct_print` walker emits the no-prefix `{…}` form bun
        // uses for object literals.
        if meta.class_name.starts_with("__anon_struct_") {
            continue;
        }
        let class_tag = (i + 1) as u32;
        let name_bytes = meta.class_name.as_bytes().to_vec();
        let name_len = meta.class_name.chars().count() as u32;
        let sym = format!("__torajs_class_name_str_{class_tag}");
        strings.push(UserStringEntry {
            sym: sym.clone(),
            bytes: name_bytes,
            is_latin1: true,
            length: name_len,
            kind: UserStringKind::RawBytes,
        });
        class_names.push(UserClassNameEntry {
            class_tag,
            name_ptr_sym: sym,
            name_len,
        });
    }
    class_names
}

/// Pair each `fn_name_globals` entry with its existing
/// `__torajs_str_dyn_<sid>` alias from the strings table (interned
/// upstream at SSA-lower time).
pub fn build_fn_name_globals(ssa_module: &Module) -> Vec<UserFnNameEntry> {
    ssa_module
        .fn_name_globals
        .iter()
        .map(|e| UserFnNameEntry {
            fn_addr_sym: format!("__torajs_fn_{}", e.fn_id.0),
            name_ptr_sym: format!("__torajs_str_dyn_{}", e.name_sid.0),
            name_len: e.name.chars().count() as u32,
            arity: e.arity,
            src_ptr_sym: e.src_sid.map(|sid| format!("__torajs_str_dyn_{}", sid.0)),
            src_len: e.src_len,
        })
        .collect()
}
