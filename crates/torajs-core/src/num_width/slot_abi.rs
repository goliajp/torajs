//! F5 — vtable-slot ABI hookups for the width analysis: every body a
//! vtable slot can resolve to shares one signature width (one slot,
//! one ABI). Split out of `container.rs` (rotation 507): that file
//! answers "which containers share a lattice point", this one "which
//! method bodies share a slot". Two entry shapes — the `__dispatch_<M>`
//! stub (single-chain names) and the bare `method_index` slot (every
//! overridden name, dispatcher or not).

use super::{Analysis, SlotKeyId};

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
                a.uf.union(&a.keys, a.keys.ret(f), a.keys.ret(&cm));
                // Positional params align 1:1 (both lists start with
                // `__this`, which stays out of the width domain).
                for (dp, cp) in d_params.iter().zip(cm_params.iter()).skip(1) {
                    a.uf.union(&a.keys, a.keys.param(f, dp), a.keys.param(&cm, cp));
                }
            }
        }
    }
    slot_unions(a);
    iface_unions(a);
}

/// One interface slot, one ABI, across every hierarchy: a call through
/// an interface reaches whichever class's witness thunk the receiver's
/// row holds, and enters it with the literal thunk's signature. The
/// rows of a face union with each other and with the member's own
/// annotation point — the fn-type class for a call face, the
/// interface's field for a get / set face — which is what the call
/// site's arguments and result are read against.
fn iface_unions(a: &mut Analysis) {
    use crate::iface_witness::{FaceKind, class_thunk, face_kind, lit_thunk};
    let ast = a.ast;
    for (iface, face) in &ast.iface_slots.faces {
        let lit = lit_thunk(iface, face);
        let Some(head) = a.fn_params.get(&lit).cloned() else {
            continue;
        };
        let classes = ast.iface_slots.witnesses.get(iface);
        for c in classes.into_iter().flatten() {
            let row = class_thunk(c, iface, face);
            let Some(ps) = a.fn_params.get(&row).cloned() else {
                continue;
            };
            a.uf.union(&a.keys, a.keys.ret(&lit), a.keys.ret(&row));
            for (hp, rp) in head.iter().zip(ps.iter()).skip(1) {
                a.uf.union(&a.keys, a.keys.param(&lit, hp), a.keys.param(&row, rp));
            }
        }
        let (key, kind) = face_kind(face);
        let Some(ann) = member_ann(ast, iface, key) else {
            continue;
        };
        let point = match kind {
            FaceKind::Call => match super::fn_type_canon(&ann) {
                Some(canon) => a.keys.class(&canon),
                None => continue,
            },
            FaceKind::Get | FaceKind::Set => a.keys.field(a.keys.class(iface), key),
            FaceKind::Has => continue,
        };
        let mut pairs: Vec<(SlotKeyId, SlotKeyId)> = Vec::new();
        match kind {
            FaceKind::Call => {
                let proj = |p: &str| a.keys.field(point, p);
                pairs.push((proj("__ret"), a.keys.ret(&lit)));
                for (i, p) in head.iter().enumerate().skip(1) {
                    let k = proj(&format!("__p{}", i - 1));
                    pairs.push((k, a.keys.param(&lit, p)));
                }
            }
            FaceKind::Get => pairs.push((point, a.keys.ret(&lit))),
            FaceKind::Has => {}
            FaceKind::Set => {
                if let Some(v) = head.get(1) {
                    pairs.push((point, a.keys.param(&lit, v)));
                }
            }
        }
        for (x, y) in pairs {
            a.mark_containerish(x);
            a.uf.union(&a.keys, x, y);
        }
    }
}

/// The annotation `iface` declares `key` with.
fn member_ann(ast: &crate::ast::Ast, iface: &str, key: &str) -> Option<String> {
    crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .find_map(|s| match s {
            crate::ast::Stmt::TypeDecl { name, fields, .. } if name == iface => fields
                .iter()
                .find(|(k, _)| *k == *key)
                .map(|(_, ann)| ann.clone()),
            _ => None,
        })
}

/// Rotation 650 — a relay shell shares its target's signature width.
/// `__forward_<t>` / `__fwdrecv_<t>` is minted as `t`'s own face
/// (`forwarder_synth`: the shell's params are the target's declared
/// ones, its body one forwarding call), and the boxed-entry redirect
/// (`ssa_lower_boxed_entry::relay`) enters `t` directly only when the
/// two SSA faces match slot for slot. The width analysis saw them as
/// two unrelated functions: a target param widened to F64 by its own
/// body left the shell's twin at I64, the faces no longer matched,
/// and every any-world call fell back to the shell body's static
/// forwarding call — which packs the declared count, so `(t as
/// any)(1, 2, 3, 4)` answered `arguments.length` 3 (silent). Each
/// shell param unions with the target param the forwarding call
/// feeds it to (positional over the target's visible params — the
/// injected `__torajs_argv` never appears in the call), and the
/// returns union too.
pub(super) fn relay_unions(a: &mut Analysis) {
    use crate::ast::{Expr, Stmt};
    let mut pairs: Vec<(SlotKeyId, SlotKeyId)> = Vec::new();
    for s in &a.ast.stmts {
        let Stmt::FnDecl { name, body, .. } = s else {
            continue;
        };
        if !name.starts_with("__forward_") && !name.starts_with("__fwdrecv_") {
            continue;
        }
        let [Stmt::Return(Some(call))] = body.as_slice() else {
            continue;
        };
        let Expr::Call { callee, args } = a.ast.get_expr(*call) else {
            continue;
        };
        let Expr::Ident(target) = a.ast.get_expr(*callee) else {
            continue;
        };
        let (Some(relay_params), Some(target_params)) =
            (a.fn_params.get(name), a.fn_params.get(target))
        else {
            continue;
        };
        let visible: Vec<&String> = target_params
            .iter()
            .filter(|p| p.as_str() != "__torajs_argv")
            .collect();
        pairs.push((a.keys.ret(name), a.keys.ret(target)));
        for (arg, tp) in args.iter().zip(visible.iter()) {
            // A rest param forwards as `...xs` (`forwarder_synth`);
            // it shares the target's rest slot the same way — left
            // out, an any-call site that also reaches an unrelated
            // callee (`call(g1); call(gr)` through one `f(6, 7)`)
            // widened the shell's `xs` through the site's `__p0`
            // projection while the target's stayed I64, and the
            // shell's `gr(...xs)` died on the element-kind check.
            let p = match a.ast.get_expr(*arg) {
                Expr::Ident(p) => p,
                Expr::Spread { expr } => match a.ast.get_expr(*expr) {
                    Expr::Ident(p) => p,
                    _ => continue,
                },
                _ => continue,
            };
            if !relay_params.contains(p) {
                continue;
            }
            pairs.push((a.keys.param(name, p), a.keys.param(target, tp)));
        }
    }
    for (x, y) in pairs {
        a.uf.union(&a.keys, x, y);
    }
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
        a.uf.union(&a.keys, a.keys.ret(&x), a.keys.ret(&y));
        let xp = a.fn_params[&x].clone();
        let yp = a.fn_params[&y].clone();
        for (dp, cp) in xp.iter().zip(yp.iter()).skip(1) {
            a.uf.union(&a.keys, a.keys.param(&x, dp), a.keys.param(&y, cp));
        }
    }
}
