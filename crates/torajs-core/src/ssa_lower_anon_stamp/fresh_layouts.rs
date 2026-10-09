//! The `ClassLayoutMeta` rows of the tags Pass 2 minted.

use std::collections::HashMap;

use crate::ast::PropKey;
use crate::ssa;

use super::AnonStampPoolCell;

/// tag, layout, the generic class it re-targets, and the creation order
type LayoutRow<'a> = (u32, ssa::StructId, Option<&'a String>, Option<&'a [u32]>);

/// Append a `ClassLayoutMeta` row per Pass 2 fresh tag recorded in
/// `pool` to `class_layouts` — plain anon sids and generic-factory
/// rows (405-03) share one tag counter, so the two lists merge
/// sorted by tag to keep the runtime indexed view dense
/// (`class_tag - 1` keeps holding).
///
/// `struct_layouts` is the post-Pass-2 vec — every fresh sid index
/// is now in-range. `generic_methods` maps a generic class to its
/// re-targeted method table (built by `populate_class_layouts`).
pub fn append_fresh_class_layouts(
    pool: &AnonStampPoolCell,
    struct_layouts: &[Vec<(PropKey, crate::ssa::Type)>],
    generic_methods: &HashMap<String, Vec<crate::ssa::MethodMetaSpec>>,
    class_layouts: &mut Vec<crate::ssa::ClassLayoutMeta>,
) {
    let p = pool.borrow();
    let mut rows: Vec<LayoutRow<'_>> = p
        .fresh_sids()
        .iter()
        .map(|sid| (p.sid_to_tag[sid], *sid, None, None))
        .chain(
            p.generic_rows()
                .iter()
                .map(|(tag, sid, base)| (*tag, *sid, Some(base), None)),
        )
        .chain(
            p.perm_rows
                .iter()
                .map(|(tag, sid, order)| (*tag, *sid, None, Some(order.as_slice()))),
        )
        .collect();
    rows.sort_by_key(|(tag, _, _, _)| *tag);
    for (_, sid, base, order) in rows {
        let sid_idx = sid.0 as usize;
        let layout = &struct_layouts[sid_idx];
        let identity: Vec<u32> = (0..layout.len() as u32).collect();
        let mut child_offsets: Vec<u32> = Vec::new();
        let mut field_metadata: Vec<crate::ssa::FieldMetaSpec> = Vec::new();
        // rows follow the creation order; each keeps its own slot offset
        for &i in order.unwrap_or(&identity) {
            let (fname, fty) = &layout[i as usize];
            let off = crate::ssa_lower::OBJ_HEADER_SIZE as u32 + i * 8;
            if fty.is_refcounted() {
                child_offsets.push(off);
            }
            field_metadata.push(crate::ssa::FieldMetaSpec {
                name: fname.clone(),
                offset: off,
                type_tag: crate::ssa::field_type_tag_of(*fty),
            });
        }
        // 404-01 / 405-03 — a generic factory's row keeps its
        // per-specialization LAYOUT but wears the class identity
        // (name + re-targeted method table).
        let (class_name, methods) = match base {
            Some(base) => (
                base.clone(),
                generic_methods.get(base).cloned().unwrap_or_default(),
            ),
            None => (format!("__anon_struct_{sid_idx}"), Vec::new()),
        };
        class_layouts.push(crate::ssa::ClassLayoutMeta {
            class_name,
            child_offsets,
            field_metadata,
            is_named: false,
            is_generic: base.is_some(),
            methods,
        });
    }
}
