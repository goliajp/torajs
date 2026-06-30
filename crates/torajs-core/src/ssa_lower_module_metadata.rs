//! Module-metadata globals builders drained from the tail of
//! `lower_inner` (chunk-331 of the lower_inner RFC decomp, after Pass
//! 0.5 / Pass 1 / Intrinsics-table siblings in chunks 328-330):
//!
//! * `populate_vtables` — T-24 per-class vtable globals. Slot order
//!   matches `ast.method_index` (sorted-by-name); for each class C,
//!   slot i resolves to `__cm_<X>__M[i]` where X is the deepest
//!   ancestor of C (incl. itself) with an own impl. Classes that
//!   don't appear in any chain method's MRO still get an empty
//!   vtable so the layout stays uniform.
//!
//! * `populate_class_layouts` — T-26.C named classes + W-J Phase A0
//!   anonymous structs. Both produce `ClassLayoutMeta` rows with
//!   per-field offsets, refcounted-children list, and per-field type
//!   tags driving the runtime's gOPD / Object.keys / inspect.rs /
//!   cycle-collector substrates. W-J Phase A1's `append_fresh_*` for
//!   Pass-2-fresh sids still lives in `ssa_lower_anon_stamp` —
//!   `lower_inner` calls it directly after this sibling returns.

use std::collections::HashMap;

use crate::ssa::{self, ClassLayoutMeta, FieldMetaSpec, Module, Type, VtableGlobal};
use crate::ssa_lower::OBJ_HEADER_SIZE;

pub(crate) fn populate_vtables(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    module: &mut Module,
) {
    if ast.method_index.is_empty() {
        return;
    }
    let n_methods = ast.method_index.len();
    // Reverse method_index → ordered method names by slot.
    let mut methods_by_slot: Vec<&str> = vec![""; n_methods];
    for (m_name, idx) in &ast.method_index {
        methods_by_slot[*idx as usize] = m_name.as_str();
    }
    let mut class_names: Vec<&String> = ast.class_parents.keys().collect();
    class_names.sort();
    for cname in class_names {
        let mut fn_ids: Vec<Option<ssa::FuncId>> = Vec::with_capacity(n_methods);
        for &m_name in &methods_by_slot {
            let mut found: Option<ssa::FuncId> = None;
            let mut cur: Option<String> = Some(cname.clone());
            let mut depth = 0u32;
            while let Some(name) = cur {
                if depth > 64 {
                    break;
                }
                let candidate = format!("__cm_{name}__{m_name}");
                if let Some(fid) = fn_table.get(&candidate) {
                    found = Some(*fid);
                    break;
                }
                cur = ast.class_parents.get(&name).and_then(|p| p.clone());
                depth += 1;
            }
            fn_ids.push(found);
        }
        module.vtable_globals.push(VtableGlobal {
            class_name: cname.clone(),
            fn_ids,
        });
    }
}

pub(crate) fn populate_class_layouts(
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    module: &mut Module,
) {
    // T-26.C — named-class metadata, walked in `class_name_to_tag`
    // order so the resulting Vec lines up with the runtime's index
    // arithmetic (cycle collector indexes class_layouts via
    // `class_tag - 1`). Class instances live behind a 24-byte object
    // header so field i is at `OBJ_HEADER_SIZE + i*8`. Non-class types
    // (anonymous `type X = {...}` aliases) get tag 0 and are
    // excluded — cycle detection on them needs heap-header-keyed sid
    // lookup as a follow-up.
    let mut class_names_by_tag: Vec<(&String, u32)> =
        class_name_to_tag.iter().map(|(n, t)| (n, *t)).collect();
    class_names_by_tag.sort_by_key(|(_, t)| *t);
    for (cname, _tag) in &class_names_by_tag {
        let sid = match module.struct_layouts.iter().enumerate().find_map(|(i, _)| {
            aliases.get(*cname).and_then(|t| match t {
                Type::Obj(s) if s.0 as usize == i => Some(i),
                _ => None,
            })
        }) {
            Some(i) => i,
            None => continue,
        };
        let layout = &module.struct_layouts[sid];
        let mut child_offsets: Vec<u32> = Vec::new();
        let mut field_metadata: Vec<FieldMetaSpec> = Vec::new();
        for (i, (fname, fty)) in layout.iter().enumerate() {
            let off = OBJ_HEADER_SIZE as u32 + (i as u32) * 8;
            if fty.is_refcounted() {
                child_offsets.push(off);
            }
            // W-J Phase A3: per-field metadata for the reflection
            // consumers (Phase B `gOPD` struct cell arm / Phase C
            // `Object.keys`/`values`/`entries` / Phase D `inspect.rs`
            // Tag::Obj walker). Carried through to Phase A3b's
            // `.__class_fields_<i>` rodata emit.
            field_metadata.push(FieldMetaSpec {
                name: fname.clone(),
                offset: off,
                type_tag: ssa::field_type_tag_of(*fty),
            });
        }
        module.class_layouts.push(ClassLayoutMeta {
            class_name: (*cname).clone(),
            child_offsets,
            field_metadata,
        });
    }

    // W-J Phase A0 (RFC 20260614-w-j-struct-reflect §3) — anonymous
    // ObjectLit struct also registers a ClassLayoutMeta entry so the
    // downstream reflection substrate (Phase B `gOPD` struct cell arm /
    // Phase C `Object.keys`/`values`/`entries` / Phase D `inspect.rs`
    // Tag::Obj walker) can look up field metadata by `class_tag@+8`.
    //
    // A0 keeps stamp paths unchanged — `class_tag@+8` continues to be
    // 0 for ObjectLit, so these new entries are dead from the cycle
    // collector's perspective; the stamp is wired in Phase A1. The
    // only observable here is `__torajs_n_class_layouts` count grows
    // by anonymous-only sid count, validating that the dyld
    // chain-fixup substrate scales with entry growth.
    let named_sids: std::collections::HashSet<ssa::StructId> = class_name_to_tag
        .keys()
        .filter_map(|cname| match aliases.get(cname) {
            Some(Type::Obj(sid)) => Some(*sid),
            _ => None,
        })
        .collect();
    let layouts = module.struct_layouts.clone();
    for (sid_idx, layout) in layouts.iter().enumerate() {
        let sid = ssa::StructId(sid_idx as u32);
        if named_sids.contains(&sid) {
            continue;
        }
        let mut child_offsets: Vec<u32> = Vec::new();
        let mut field_metadata: Vec<FieldMetaSpec> = Vec::new();
        for (i, (fname, fty)) in layout.iter().enumerate() {
            let off = OBJ_HEADER_SIZE as u32 + (i as u32) * 8;
            if fty.is_refcounted() {
                child_offsets.push(off);
            }
            // W-J Phase A3 — same per-field metadata population as the
            // named-class branch above. Anonymous structs share the
            // reflection consumer surface (`{a:1}` as `gOPD` target,
            // `Object.keys({a:1})` etc.).
            field_metadata.push(FieldMetaSpec {
                name: fname.clone(),
                offset: off,
                type_tag: ssa::field_type_tag_of(*fty),
            });
        }
        module.class_layouts.push(ClassLayoutMeta {
            class_name: format!("__anon_struct_{sid_idx}"),
            child_offsets,
            field_metadata,
        });
    }
}
