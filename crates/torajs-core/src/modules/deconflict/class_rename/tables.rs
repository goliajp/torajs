//! Knife D — table-row ownership and migration for a class rename.
//!
//! The parent orchestrates the rename and rewrites baked spellings;
//! this file owns the question "which side-table rows belong to the
//! renamed lib class, and where do they go" — the snapshot/diff
//! machinery `load_lib_section` drives and the per-table MOVE /
//! restore / gated-COPY rules (ownership mechanisms 1-3, parent
//! module doc).

use super::swap_prefix;
use crate::ast::{Ast, PropKey, Stmt};
use std::collections::{HashMap, HashSet};

/// Pre-parse snapshot of the name-keyed class tables — taken by
/// `load_lib_section` right before the lib's `parse_into`, diffed
/// right after, so the census knows which rows the LIB wrote.
pub(in crate::modules) struct ClassTableSnapshot {
    explicit: HashSet<String>,
    synth: HashSet<String>,
    parent_args: HashMap<String, Vec<String>>,
    vis: HashMap<(String, PropKey), crate::ast::Visibility>,
    readonly: HashSet<(String, PropKey)>,
}

pub(in crate::modules) fn snapshot_class_tables(ast: &Ast) -> ClassTableSnapshot {
    ClassTableSnapshot {
        explicit: ast.explicit_ctor_classes.clone(),
        synth: ast.field_init_synth_ctors.clone(),
        parent_args: ast.class_parent_type_args.clone(),
        vis: ast.member_visibility.clone(),
        readonly: ast.readonly_fields.clone(),
    }
}

/// Which rows the lib's parse added (`*_new`) or overwrote
/// (`*_changed`, remembering the entry's original value so the
/// rename can restore it).
pub(in crate::modules) struct LibTableDelta {
    explicit_new: HashSet<String>,
    synth_new: HashSet<String>,
    parent_args_new: HashSet<String>,
    parent_args_changed: HashMap<String, Vec<String>>,
    vis_new: HashSet<(String, PropKey)>,
    vis_changed: HashMap<(String, PropKey), crate::ast::Visibility>,
    readonly_new: HashSet<(String, PropKey)>,
}

pub(in crate::modules) fn diff_class_tables(ast: &Ast, snap: &ClassTableSnapshot) -> LibTableDelta {
    LibTableDelta {
        explicit_new: ast
            .explicit_ctor_classes
            .difference(&snap.explicit)
            .cloned()
            .collect(),
        synth_new: ast
            .field_init_synth_ctors
            .difference(&snap.synth)
            .cloned()
            .collect(),
        parent_args_new: ast
            .class_parent_type_args
            .keys()
            .filter(|k| !snap.parent_args.contains_key(*k))
            .cloned()
            .collect(),
        parent_args_changed: snap
            .parent_args
            .iter()
            .filter(|(k, v)| ast.class_parent_type_args.get(*k).is_some_and(|c| c != *v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        vis_new: ast
            .member_visibility
            .keys()
            .filter(|k| !snap.vis.contains_key(*k))
            .cloned()
            .collect(),
        vis_changed: snap
            .vis
            .iter()
            .filter(|(k, v)| ast.member_visibility.get(*k).is_some_and(|c| c != *v))
            .map(|(k, v)| (k.clone(), *v))
            .collect(),
        readonly_new: ast
            .readonly_fields
            .difference(&snap.readonly)
            .cloned()
            .collect(),
    }
}

/// Tables whose rows carry an ExprId — ownership is exact by arena
/// offset, no delta needed.
pub(super) fn migrate_exprid_tables(ast: &mut Ast, lib_expr_offset: usize, old: &str, new: &str) {
    for (eid, cls) in ast.static_this_sites.iter_mut() {
        if (eid.0 as usize) >= lib_expr_offset && cls == old {
            *cls = new.to_string();
        }
    }
    let moved: Vec<(String, String)> = ast
        .class_computed_keys
        .keys()
        .filter(|k| k.0 == old && ast.class_computed_keys[*k].0 as usize >= lib_expr_offset)
        .cloned()
        .collect();
    for k in moved {
        let v = ast.class_computed_keys.remove(&k).unwrap();
        ast.class_computed_keys.insert((new.to_string(), k.1), v);
    }
    for row in ast.class_computed_static_fields.iter_mut() {
        if row.0 == old && (row.2.0 as usize) >= lib_expr_offset {
            row.0 = new.to_string();
        }
    }
}

/// Name-keyed tables — split by the parse-time snapshot diff, with a
/// structure gate on the COPY fallback (module doc, mechanism 2+3).
pub(super) fn migrate_name_keyed_tables(
    ast: &mut Ast,
    decl: &Stmt,
    old: &str,
    new: &str,
    delta: &LibTableDelta,
) {
    let Stmt::ClassDecl {
        parent,
        ctor,
        fields,
        methods,
        static_methods,
        static_init,
        ..
    } = (match decl {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => inner.as_ref(),
        other => other,
    })
    else {
        return;
    };
    // explicit / synth ctor sets — exclusive; the diff decides when
    // it can, same-set membership decides the both-sides-agree case.
    if ctor.is_some() {
        if delta.explicit_new.contains(old) {
            ast.explicit_ctor_classes.remove(old);
            ast.explicit_ctor_classes.insert(new.to_string());
        } else if delta.synth_new.contains(old) {
            ast.field_init_synth_ctors.remove(old);
            ast.field_init_synth_ctors.insert(new.to_string());
        } else if ast.explicit_ctor_classes.contains(old) {
            ast.explicit_ctor_classes.insert(new.to_string());
        } else if ast.field_init_synth_ctors.contains(old) {
            ast.field_init_synth_ctors.insert(new.to_string());
        }
    }
    if parent.is_some() {
        if delta.parent_args_new.contains(old) {
            if let Some(v) = ast.class_parent_type_args.remove(old) {
                ast.class_parent_type_args.insert(new.to_string(), v);
            }
        } else if let Some(entry_orig) = delta.parent_args_changed.get(old) {
            if let Some(v) = ast
                .class_parent_type_args
                .insert(old.to_string(), entry_orig.clone())
            {
                ast.class_parent_type_args.insert(new.to_string(), v);
            }
        } else if let Some(v) = ast.class_parent_type_args.get(old).cloned() {
            ast.class_parent_type_args.insert(new.to_string(), v);
        }
    }
    // (class, member)-keyed rows. The member's own spelling migrates
    // with the class when it is a `__priv_<C>__` bake.
    let has_member = |m: &PropKey| {
        methods
            .iter()
            .chain(static_methods.iter())
            .any(|cm| cm.name == *m)
            || fields.iter().any(|(n, _)| n == m)
            || static_init.iter().any(|si| match si {
                crate::ast::StaticInit::Field(f) => f.name == *m,
                crate::ast::StaticInit::Block(_) => false,
            })
    };
    let (pre_old, pre_new) = (super::priv_prefix(old), super::priv_prefix(new));
    let vis_keys: Vec<(String, PropKey)> = ast
        .member_visibility
        .keys()
        .filter(|k| k.0 == old)
        .cloned()
        .collect();
    for k in vis_keys {
        let new_m =
            k.1.as_str()
                .and_then(|s| swap_prefix(s, &pre_old, &pre_new))
                .map_or_else(|| k.1.clone(), PropKey::from);
        if delta.vis_new.contains(&k) {
            if let Some(v) = ast.member_visibility.remove(&k) {
                ast.member_visibility.insert((new.to_string(), new_m), v);
            }
        } else if let Some(orig) = delta.vis_changed.get(&k) {
            if let Some(v) = ast.member_visibility.insert(k.clone(), *orig) {
                ast.member_visibility.insert((new.to_string(), new_m), v);
            }
        } else if has_member(&k.1) {
            if let Some(v) = ast.member_visibility.get(&k).copied() {
                ast.member_visibility.insert((new.to_string(), new_m), v);
            }
        }
    }
    let ro_keys: Vec<(String, PropKey)> = ast
        .readonly_fields
        .iter()
        .filter(|k| k.0 == old)
        .cloned()
        .collect();
    for k in ro_keys {
        let new_m =
            k.1.as_str()
                .and_then(|s| swap_prefix(s, &pre_old, &pre_new))
                .map_or_else(|| k.1.clone(), PropKey::from);
        if delta.readonly_new.contains(&k) {
            ast.readonly_fields.remove(&k);
            ast.readonly_fields.insert((new.to_string(), new_m));
        } else if has_member(&k.1) {
            ast.readonly_fields.insert((new.to_string(), new_m));
        }
    }
    // Derived method spellings in the fn tables (`__cm_<C>__` /
    // `__sm_<C>__` / `__cm_gen_<C>__`): COPY — a leftover row with no
    // decl is a no-op (knife-A posture), and same-named methods on
    // same-named classes derive identical strings, so the copy is
    // right for both sides.
    let derived = [
        format!("__cm_{old}__"),
        format!("__sm_{old}__"),
        format!("__cm_gen_{old}__"),
    ];
    let derived_new = [
        format!("__cm_{new}__"),
        format!("__sm_{new}__"),
        format!("__cm_gen_{new}__"),
    ];
    for (pre, pre_n) in derived.iter().zip(derived_new.iter()) {
        let hits: Vec<String> = ast
            .async_fns
            .iter()
            .filter_map(|n| swap_prefix(n, pre, pre_n))
            .collect();
        ast.async_fns.extend(hits);
        let hits: Vec<String> = ast
            .async_generator_fns
            .iter()
            .filter_map(|n| swap_prefix(n, pre, pre_n))
            .collect();
        ast.async_generator_fns.extend(hits);
        let hits: Vec<(String, usize)> = ast
            .gen_param_destr_prefix
            .iter()
            .filter_map(|(n, &v)| swap_prefix(n, pre, pre_n).map(|nn| (nn, v)))
            .collect();
        ast.gen_param_destr_prefix.extend(hits);
    }
}
