//! F5 — vtable-slot ABI hookups for the width analysis: every body a
//! vtable slot can resolve to shares one signature width (one slot,
//! one ABI). Split out of `container.rs` (rotation 507): that file
//! answers "which containers share a lattice point", this one "which
//! method bodies share a slot". Two entry shapes — the `__dispatch_<M>`
//! stub (single-chain names) and the bare `method_index` slot (every
//! overridden name, dispatcher or not).

use super::{Analysis, SlotKey};

/// F5 — vtable-slot ABI hookups: the synthetic `__dispatch_<M>` fn is
/// lowered as a tag-switch over every owner's `__cm_<C>__<M>`, so the
/// dispatcher and ALL owners must share one signature width (one
/// vtable slot, one ABI). The dispatcher's AST stub only forwards to
/// the base owner — which an abstract base doesn't even emit — so
/// without these unions `Ret(__dispatch_<M>)` is an orphan class and
/// a f64-possible override's return reads back as a garbage integer
/// through the dispatch face.
pub(super) fn dispatch_unions(a: &mut Analysis) {
    let mut fn_names: Vec<String> = a.fn_params.keys().cloned().collect();
    fn_names.sort();
    for f in &fn_names {
        let Some(m_name) = f.strip_prefix("__dispatch_") else {
            continue;
        };
        // A MONO dispatcher (`__dispatch_area$$_number`) shares its
        // vtable slot with every owner's impl under BOTH spellings:
        // the same-suffix mono (`__cm_Shape__area$$_number`, the
        // generic base) and the bare one (`__cm_Circle__area`, a
        // non-generic overrider) — the suffix rides the name's tail,
        // so the bare-name overrider only unions through here.
        let (bare_m, suffix) = m_name
            .split_once("$$")
            .map(|(b, s)| (b, format!("$${s}")))
            .unwrap_or((m_name, String::new()));
        let d_params = a.fn_params[f].clone();
        for c in a.classes.clone() {
            for cm in [
                format!("__cm_{c}__{bare_m}{suffix}"),
                format!("__cm_{c}__{bare_m}"),
            ] {
                let Some(cm_params) = a.fn_params.get(&cm).cloned() else {
                    continue;
                };
                a.uf.union(&SlotKey::Ret(f.clone()), &SlotKey::Ret(cm.clone()));
                // Positional params align 1:1 (both lists start with
                // `__this`, which stays out of the width domain).
                for (dp, cp) in d_params.iter().zip(cm_params.iter()).skip(1) {
                    a.uf.union(
                        &SlotKey::Param(f.clone(), dp.clone()),
                        &SlotKey::Param(cm.clone(), cp.clone()),
                    );
                }
            }
        }
    }
    slot_unions(a);
}

/// Rotation 507 — the same one-slot-one-ABI rule for every OVERRIDDEN
/// name, dispatcher or not: a name an unrelated class also declares
/// gets no `__dispatch_` stub (its Member-shape sites read the slot
/// from the sibling-dispatch lane), so the loop above never sees it,
/// and an f64-possible override's return read back as garbage bits
/// through the slot (probe: `-this.id` printed 4380426256). Owners
/// union per hierarchy ROOT — the unrelated declarer's row fills the
/// same slot with its own body under its own signature.
fn slot_unions(a: &mut Analysis) {
    let mut names: Vec<&String> = a.ast.method_index.keys().collect();
    names.sort();
    let mut unions: Vec<(String, String)> = Vec::new();
    for m in names {
        let mut first_by_root: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for c in &a.classes {
            let prefix = format!("__cm_{c}__{m}");
            let mut cms: Vec<&String> = a
                .fn_params
                .keys()
                .filter(|k| {
                    k.strip_prefix(prefix.as_str())
                        .is_some_and(|r| r.is_empty() || r.starts_with("$$"))
                })
                .collect();
            cms.sort();
            let root = crate::ast::hierarchy_root(a.ast, c);
            for cm in cms {
                match first_by_root.get(&root) {
                    Some(head) => unions.push((head.clone(), cm.clone())),
                    None => {
                        first_by_root.insert(root.clone(), cm.clone());
                    }
                }
            }
        }
    }
    for (x, y) in unions {
        a.uf.union(&SlotKey::Ret(x.clone()), &SlotKey::Ret(y.clone()));
        let xp = a.fn_params[&x].clone();
        let yp = a.fn_params[&y].clone();
        for (dp, cp) in xp.iter().zip(yp.iter()).skip(1) {
            a.uf.union(
                &SlotKey::Param(x.clone(), dp.clone()),
                &SlotKey::Param(y.clone(), cp.clone()),
            );
        }
    }
}
