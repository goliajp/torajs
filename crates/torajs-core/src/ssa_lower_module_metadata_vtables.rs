//! T-24 per-class vtable globals, split out of
//! `ssa_lower_module_metadata` (that file answers the class-layout
//! metadata; this one the dispatch tables). Slot order matches
//! `ast.method_index` (sorted-by-name); for each class C, slot i
//! resolves to `__cm_<X>__M[i]` where X is the deepest ancestor of C
//! (incl. itself) with an own impl. Classes that don't appear in any
//! chain method's MRO still get an empty vtable so the layout stays
//! uniform.

use std::collections::HashMap;

use crate::ssa::{self, Module, VtableGlobal};

pub(crate) fn populate_vtables(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    module: &mut Module,
) {
    if ast.method_index.is_empty() && ast.iface_slots.is_empty() {
        return;
    }
    let n_methods = ast.method_index.len();
    // Reverse method_index → ordered method names by slot.
    let mut methods_by_slot: Vec<&str> = vec![""; n_methods];
    for (m_name, idx) in &ast.method_index {
        methods_by_slot[*idx as usize] = m_name.as_str();
    }
    let mut class_names: Vec<String> = ast.class_parents.keys().cloned().collect();
    // A GENERIC class's instances come from mono factories
    // (`__new_<Base>$$<suffix>`) and need a vtable row per
    // specialization — the bare row's slots all miss (a generic
    // method's bare `__cm_` name never enters fn_table), so an
    // instance wearing it would CallIndirect through null. Slot
    // resolution walks the same ancestor chain but tries the
    // suffixed spelling first (`__cm_<X>__<M>$$<suffix>` — the
    // stub-body recheck seeded it wherever a call site exists),
    // then the bare one (non-generic ancestors / overriders).
    let mut mono_rows: Vec<(String, String)> = fn_table
        .keys()
        .filter_map(|k| {
            let rest = k.strip_prefix("__new_")?;
            let (base, _) = rest.split_once("$$")?;
            ast.class_parents
                .contains_key(base)
                .then(|| (rest.to_string(), rest[base.len()..].to_string()))
        })
        .collect();
    mono_rows.sort();
    class_names.sort();
    let rows: Vec<(String, String)> = class_names
        .into_iter()
        .map(|c| (c, String::new()))
        .chain(mono_rows)
        .collect();
    for (cname, suffix) in rows {
        let mut fn_ids: Vec<Option<ssa::FuncId>> = Vec::with_capacity(n_methods);
        for &m_name in &methods_by_slot {
            let mut found: Option<ssa::FuncId> = None;
            let base_name = cname
                .split_once("$$")
                .map(|(b, _)| b.to_string())
                .unwrap_or_else(|| cname.clone());
            let mut cur: Option<String> = Some(base_name);
            let mut depth = 0u32;
            while let Some(name) = cur {
                if depth > 64 {
                    break;
                }
                let suffixed = format!("__cm_{name}__{m_name}{suffix}");
                let bare = format!("__cm_{name}__{m_name}");
                if let Some(fid) = fn_table.get(&suffixed).or_else(|| fn_table.get(&bare)) {
                    found = Some(*fid);
                    break;
                }
                cur = ast.class_parents.get(&name).and_then(|p| p.clone());
                depth += 1;
            }
            fn_ids.push(found);
        }
        // a class's witness rows are its own (the thunks are minted
        // per class, inheritance already resolved inside them)
        for (iface, face) in &ast.iface_slots.faces {
            let thunk = crate::iface_witness::class_thunk(&cname, iface, face);
            fn_ids.push(fn_table.get(&thunk).copied());
        }
        module.vtable_globals.push(VtableGlobal {
            class_name: cname,
            fn_ids,
        });
    }
    push_lit_rows(ast, fn_table, module, n_methods);
}

/// One row per object-literal layout that can stand in for an
/// interface (`ssa_lower_iface::lit_row_ifaces` — the same test the
/// literal's allocation makes): its method slots stay empty, its
/// interface slots hold the literal thunks.
fn push_lit_rows(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    module: &mut Module,
    n_methods: usize,
) {
    if ast.iface_slots.is_empty() {
        return;
    }
    for sid in 0..module.struct_layouts.len() {
        let ifaces = crate::ssa_lower_iface::lit_row_ifaces(ast, &module.struct_layouts[sid]);
        if ifaces.is_empty() {
            continue;
        }
        let mut fn_ids: Vec<Option<ssa::FuncId>> = vec![None; n_methods];
        for (iface, face) in &ast.iface_slots.faces {
            let fid = ifaces
                .contains(iface)
                .then(|| fn_table.get(&crate::iface_witness::lit_thunk(iface, face)))
                .flatten()
                .copied();
            fn_ids.push(fid);
        }
        module.vtable_globals.push(VtableGlobal {
            class_name: crate::ssa_lower_iface::lit_row_name(ssa::StructId(sid as u32)),
            fn_ids,
        });
    }
}
