//! SSA → link-layer registry bridges built once per `tr build` /
//! `tr run`. Carved out of `cmd_build.rs::build_link_config` in
//! W-J Phase A3c chunk 2 to keep `cmd_build.rs` below its file-size
//! debt ceiling after the class-name registry intern step landed.
//!
//! - `build_user_strings`: `ssa::Module.strings` → one StaticStr
//!   (`__torajs_str_lit_<i>`) entry per literal, carrying
//!   `__torajs_str_dyn_<i>` as the alias of its payload.
//! - `build_class_names`: `ssa::Module.class_layouts` →
//!   `(UserClassNameEntry, UserStringEntry)` pairs for each named
//!   class, appended to the strings vec under
//!   `__torajs_class_name_str_<tag>`.
//! - `build_fn_name_globals`: `ssa::Module.fn_name_globals` →
//!   `UserFnNameEntry` Vec keyed on existing `__torajs_str_dyn_<sid>`
//!   alias from the strings table.

use torajs_core::ssa::{FuncId, Module, Type};
use torajs_link::exec::{
    UserClassLayoutEntry, UserClassNameEntry, UserDataGlobalEntry, UserFnNameEntry,
    UserStringEntry, UserStringKind, UserVtableEntry,
};

/// One entry per literal. The Str cell (`__torajs_str_lit_<i>`) is
/// header + payload; the raw-byte readers' `__torajs_str_dyn_<i>` is
/// registered as an alias of that payload rather than emitted as a
/// second copy — a class program spent a third of its user-string
/// region on the duplicates (s3 rotation 504 census).
pub fn build_user_strings(ssa_module: &Module) -> Vec<UserStringEntry> {
    ssa_module
        .strings
        .iter()
        .enumerate()
        .map(|(i, lit)| UserStringEntry {
            sym: format!("__torajs_str_lit_{i}"),
            bytes: lit.bytes.clone(),
            is_latin1: lit.is_latin1,
            length: lit.length,
            kind: UserStringKind::StaticStr,
            payload_alias: Some(format!("__torajs_str_dyn_{i}")),
        })
        .collect()
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
            payload_alias: None,
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

/// One `UserDataGlobalEntry` per SSA data global (sym + slot size /
/// alignment from the SSA type).
pub(crate) fn build_data_globals(ssa_module: &Module) -> Vec<UserDataGlobalEntry> {
    ssa_module
        .data_globals
        .iter()
        .map(|dg| {
            let (size, align_log2) = type_slot_size_align(dg.ty);
            UserDataGlobalEntry {
                sym: dg.name.clone(),
                size,
                align_log2,
            }
        })
        .collect()
}

/// SD-4c-prereq+e8 — materialize ssa::Module.class_layouts (T-26.C
/// cycle collector metadata) into the proper in-house rodata path
/// (`__torajs_class_layouts` outer table + per-class inner
/// `.__class_offsets_<i>` globals + `__torajs_n_class_layouts`
/// count). Pre-e8 reserved two zerofill slots in `data_globals`
/// (now removed): the outer-ptr was NULL so the cycle collector
/// short-circuited on class-bearing programs. e8 lands real bytes
/// + dyld rebase via the e7b chained-fixups TextRebaseScope.
pub(crate) fn build_class_layout_entries(ssa_module: &Module) -> Vec<UserClassLayoutEntry> {
    ssa_module
        .class_layouts
        .iter()
        .map(|cl| UserClassLayoutEntry {
            child_offsets: cl.child_offsets.clone(),
            is_named: cl.is_named,
            is_generic: cl.is_generic,
            // W-J A3b — plumb FieldMetaSpec through to the link layer so
            // it can emit the per-class `.__class_fields_<i>` inner
            // global + per-field name strings + wire the outer entry's
            // field_metadata_ptr slot to the inner global's vaddr.
            fields: cl
                .field_metadata
                .iter()
                .map(|fm| torajs_link::exec::UserFieldMetaEntry {
                    name: fm.name.clone(),
                    offset: fm.offset,
                    type_tag: fm.type_tag,
                })
                .collect(),
            // 刀 4 (RFC 20260714-t262-top-clusters) — runtime class-
            // method dispatch rows; adapter fids resolve through
            // fn_vaddrs at rebase-assembly time.
            methods: cl
                .methods
                .iter()
                .map(|mm| torajs_link::exec::UserMethodMetaEntry {
                    name: mm.name.clone(),
                    adapter_fn_id: Some(mm.adapter_fid.0),
                    // Bit 0 = this-free (S2.38); bit 1 = twin-primary
                    // (404-01 — the adapter is recv-first-shaped);
                    // bit 2 = declared by this class rather than
                    // inherited (508-03 — gates the own-entry reify).
                    flags: u32::from(mm.this_free)
                        | (u32::from(mm.twin_primary) << 1)
                        | (u32::from(mm.declared_here) << 2),
                    twin_fn_id: mm.twin_adapter_fid.map(|f| f.0),
                })
                .collect(),
        })
        .collect()
}

/// vtable slots resolve via `register_fn_addr_syms`'s
/// `__torajs_fn_<i>` override (codegen's `FnAddr` convention) — see
/// `archive_emit::link_to_exec_with_archives` and the
/// `probe_vtable_link` reference.
pub(crate) fn build_vtable_globals(ssa_module: &Module) -> Vec<UserVtableEntry> {
    ssa_module
        .vtable_globals
        .iter()
        .map(|vt| UserVtableEntry {
            sym: format!("__vtable_{}", vt.class_name),
            slot_syms: vt
                .fn_ids
                .iter()
                .map(|opt| opt.map(|fid: FuncId| format!("__torajs_fn_{}", fid.0)))
                .collect(),
        })
        .collect()
}

/// SSA `Type` → `(slot size in bytes, log2 alignment)` for
/// `__DATA,__bss` placement. Heap-shaped reference types lower to a
/// single pointer at codegen, so they share the I64 8/3 slot. `Void`
/// is not allocable and panics at this layer (the SSA layer should
/// never declare a `let x: void`).
pub(crate) fn type_slot_size_align(ty: Type) -> (u32, u8) {
    match ty {
        Type::Void => panic!("DataGlobal of type Void is not allocable"),
        Type::I32 => (4, 2),
        Type::Bool => (1, 0),
        _ => (8, 3),
    }
}
