//! Accessor rows of a class's method table: the opposite-face
//! shadowing the parent-chain walk applies, and the generic-class
//! retarget that has to recover an accessor row's declaring face.

use std::collections::HashMap;

use crate::ast::{PropKey, mangle_key};
use crate::ssa;

/// The slot spelling of the opposite face of an accessor row
/// (`__getter_p` ↔ `__setter_p`); `None` for a method row.
pub(super) fn other_accessor_face(row: &PropKey) -> Option<PropKey> {
    let key = row.as_wtf8();
    if let Some(p) = key.strip_prefix("__getter_") {
        return Some(PropKey::prefixed("__setter_", p));
    }
    key.strip_prefix("__setter_")
        .map(|p| PropKey::prefixed("__getter_", p))
}

/// 404-01 — re-target a GENERIC class's method rows at the
/// receiver-polymorphic twin's all-any instance.
///
/// The `$$anywv` mono body reads its receiver at ONE specialization's
/// static offsets (its `__this` erases to `G<any>`), while an
/// instance wears whichever specialization it was built with — a
/// `G<number>` k is a raw i64 the mono misdecodes as a NaN box. The
/// twin (`__cmany_…$$anywv`) reads through GetV, which honors the
/// receiver row's per-field type tags, so it is correct under every
/// specialization. A receiver-FREE body keeps its mono adapter (it
/// touches no field, so no layout can betray it); a receiver-reading
/// body whose twin was never minted (accessor slots, super-carrying
/// bodies) drops its row — the honest no-such TypeError over a
/// misread answer.
pub(crate) fn retarget_generic_class_methods(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    boxed_entries: &HashMap<ssa::FuncId, (ssa::FuncId, ssa::SigId)>,
    base: &str,
    methods: Vec<ssa::MethodMetaSpec>,
) -> Vec<ssa::MethodMetaSpec> {
    methods
        .into_iter()
        .filter_map(|mut m| {
            // RFC 20260815 刀 5 — an accessor row's name is the slot
            // spelling (`__getter_value`), not a `__cm_` suffix; its
            // twin is keyed by the getter/setter FN name, recovered
            // through the accessor maps (walking the parent chain —
            // the row may be inherited, and the twin reads through
            // GetV so it is sound under a subclass receiver too).
            let decl = accessor_decl_for(ast, base, &m.name)
                .unwrap_or_else(|| format!("__cm_{base}__{}", mangle_key(&m.name)));
            let Some(twin) = ast.cmany_twins.get(&decl) else {
                return m.this_free.then_some(m);
            };
            let fid = fn_table.get(format!("{twin}$$anywv").as_str()).copied()?;
            let (adapter, _) = boxed_entries.get(&fid).copied()?;
            m.adapter_fid = adapter;
            m.twin_adapter_fid = None;
            m.twin_primary = true;
            Some(m)
        })
        .collect()
}

/// RFC 20260815 刀 5 — recover the declaring getter/setter FN name
/// behind an accessor slot row (`__getter_value` on class `Box` →
/// `__cm_Box__value_get`), walking `class_parents` because a resolved
/// method table merges inherited rows. `None` for a plain method row
/// or an accessor no ancestor declares.
fn accessor_decl_for(ast: &crate::ast::Ast, base: &str, slot: &PropKey) -> Option<String> {
    let (table, prop) = match slot.strip_prefix("__getter_") {
        Some(p) => (&ast.accessor_getters, p),
        None => (&ast.accessor_setters, slot.strip_prefix("__setter_")?),
    };
    let prop = PropKey::from(prop);
    let mut cur = Some(base.to_string());
    let mut depth = 0u32;
    while let Some(c) = cur {
        if depth > 64 {
            break;
        }
        if let Some(f) = table.get(&(c.clone(), prop.clone())) {
            return Some(f.clone());
        }
        cur = ast.class_parents.get(&c).and_then(|p| p.clone());
        depth += 1;
    }
    None
}
