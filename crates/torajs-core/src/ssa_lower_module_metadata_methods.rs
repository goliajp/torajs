//! Own-method collection + inherited-chain resolution for the
//! class-methods dispatch table — split out of
//! `ssa_lower_module_metadata.rs` (file-size line, rotation 296).
//! `collect_own_class_methods` maps each class to its `__cm_` rows
//! (the blade-3 twin body fid resolved alongside);
//! `resolve_class_methods` walks the parent chain and emits the
//! deduped `MethodMetaSpec` table for one class.

use std::collections::HashMap;

use crate::ssa;

/// 刀 4 (RFC 20260714-t262-top-clusters) — collect every class's OWN
/// `__cm_<C>__<m>` method bodies by scanning the fn table (NOT
/// `ast.method_index`, which only carries chain/vtable methods —
/// single-owner methods are statically rewritten and never enter it).
/// A class name may itself contain `__`, so the `<C>` boundary is
/// resolved by longest-match against the known class-name set. The
/// ctor (`__cm_<C>__ctor`) never enters — `new` is not a method call.
///
/// RFC 20260714-objlit-accessor blade 5 — an ACCESSOR body
/// (`__cm_<C>__<p>_get`) enters under the synthetic slot spelling
/// `__getter_<p>`, the same name an object-literal accessor carries in
/// the layout. Two things ride on that:
///
/// * the runtime `any` member read resolves `o.p` through it (a class
///   accessor is prototype-level, so unlike the literal's it has no
///   layout field to live in);
/// * `<p>_get` STOPS being a callable method name. It was one — probe
///   at `cd0f3caf`: `(new C() as any).b_get()` answered the getter's
///   999 while bun rejects the name outright. The mangled spelling was
///   leaking onto the user-visible method surface.
///
/// The name is taken from `ast.accessor_getters` / `accessor_setters`
/// (keyed by fn name), never by guessing at a `_get` suffix — a plain
/// method really called `b_get` is a legal method.
pub(crate) fn collect_own_class_methods(
    ast: &crate::ast::Ast,
    fn_table: &HashMap<String, ssa::FuncId>,
    class_names: &[&String],
) -> HashMap<String, Vec<(String, ssa::FuncId, Option<ssa::FuncId>)>> {
    let mut accessor_slots: HashMap<&str, String> = HashMap::new();
    for ((_, prop), fname) in &ast.accessor_getters {
        accessor_slots.insert(fname.as_str(), format!("__getter_{prop}"));
    }
    for ((_, prop), fname) in &ast.accessor_setters {
        accessor_slots.insert(fname.as_str(), format!("__setter_{prop}"));
    }
    let mut own: HashMap<String, Vec<(String, ssa::FuncId, Option<ssa::FuncId>)>> = HashMap::new();
    for (fname, &fid) in fn_table {
        let Some(rest) = fname.strip_prefix("__cm_") else {
            continue;
        };
        // A `__cmany_` twin body is itself in the fn_table but is
        // never an own method row (explicit skip — a user class
        // named `any_<X>` would otherwise collide via the shared
        // `__cm` spelling).
        if fname.starts_with("__cmany_") {
            continue;
        }
        // Longest class-name match wins (`__cm_A__b__c` with classes
        // `A` and `A__b` belongs to `A__b`).
        let mut best: Option<(&str, &str)> = None;
        for cname in class_names {
            if let Some(m) = rest.strip_prefix(&format!("{cname}__"))
                && best.is_none_or(|(b, _)| cname.len() > b.len())
            {
                best = Some((cname.as_str(), m));
            }
        }
        let Some((cname, mname)) = best else { continue };
        if mname == "ctor" || mname.is_empty() {
            continue;
        }
        let entry_name = match accessor_slots.get(fname.as_str()) {
            Some(slot) => slot.clone(),
            None => mname.to_string(),
        };
        // Blade 3 — the receiver-polymorphic twin's body fid, when
        // blade 2 minted one for this mono.
        let twin_fid = ast
            .cmany_twins
            .get(fname.as_str())
            .and_then(|twin_name| fn_table.get(twin_name.as_str()).copied());
        own.entry(cname.to_string())
            .or_default()
            .push((entry_name, fid, twin_fid));
    }
    own
}

/// 刀 4 — resolve one class's runtime-dispatchable methods: its own
/// `__cm_` bodies plus inherited ones up the parent chain (child
/// declarations shadow, the vtable walk's override semantics). Only
/// bodies with a synthesized boxed adapter survive — a synthesis
/// dropout (>8 params / unboxable type) keeps the runtime miss an
/// honest no-such TypeError.
pub(crate) fn resolve_class_methods(
    ast: &crate::ast::Ast,
    own_methods: &HashMap<String, Vec<(String, ssa::FuncId, Option<ssa::FuncId>)>>,
    boxed_entries: &HashMap<ssa::FuncId, (ssa::FuncId, ssa::SigId)>,
    this_free_fids: &std::collections::HashSet<ssa::FuncId>,
    cname: &str,
) -> Vec<ssa::MethodMetaSpec> {
    let mut out: Vec<ssa::MethodMetaSpec> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut cur: Option<String> = Some(cname.to_string());
    let mut depth = 0u32;
    while let Some(name) = cur {
        if depth > 64 {
            break;
        }
        if let Some(methods) = own_methods.get(&name) {
            for (mname, fid, twin_fid) in methods {
                if seen.contains(mname.as_str()) {
                    continue;
                }
                if let Some(&(adapter_fid, _)) = boxed_entries.get(fid) {
                    out.push(ssa::MethodMetaSpec {
                        name: mname.clone(),
                        adapter_fid,
                        this_free: this_free_fids.contains(fid),
                        twin_adapter_fid: twin_fid
                            .and_then(|t| boxed_entries.get(&t).map(|&(a, _)| a)),
                    });
                }
                // Shadow even on adapter dropout — a child decl
                // without an adapter must not expose the parent's.
                seen.insert(mname.as_str());
            }
        }
        cur = ast.class_parents.get(&name).and_then(|p| p.clone());
        depth += 1;
    }
    // Deterministic table order (HashMap iteration fed `own_methods`).
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
