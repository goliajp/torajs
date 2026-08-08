//! SuperProperty rewrite pass (§13.3.7 MakeSuperPropertyReference).
//!
//! Runs right after `rewrite_super_method_calls` (Pass 1.6) and before
//! Pass 2 (`Expr::This` rewrite). For every class with an `extends`
//! clause it walks the ctor + method bodies (instance context) and the
//! static-method bodies (static context) for the `__superbase__`
//! marker sites the parser encoded (see `super_collect_prop`) and
//! rewrites each per its role:
//!
//!   - read `super.x`, statically-declared getter on the chain →
//!     `__cm_<owner>__<x>_get(this)` (static ctx: `__sm_..._get()`),
//!     keeping the §13.3.7 receiver (`this`), which a plain member
//!     read off the prototype object could not deliver;
//!   - read `super.x` / `super[k]`, anything else → a member read off
//!     the super base: `<Parent>.prototype` for instance members, the
//!     `<Parent>` class object for statics. The runtime prototype
//!     chain handles shadowing, plain data properties added at run
//!     time (`A.prototype.p = v`), method values, and undefined;
//!   - write `super.x = v` at statement position with a statically-
//!     declared setter on the chain → `__cm_<owner>__<x>_set(this, v)`;
//!   - write `super.x = v` / `super[k] = v` otherwise → a write to
//!     `this` (§9.1.9 OrdinarySet with receiver = this: data
//!     properties land on the receiver, and the member-assign lower's
//!     own accessor dispatch on `this` covers the un-shadowed
//!     inherited-setter shape).
//!
//! Recorded approximation boundaries (loud, not silent — the marker
//! survives to the checker as an unknown ident): `super[k](...)`
//! calls (receiver would be wrong), `super.x++` (fused read/write).
//! Recorded silent boundaries: a RUNTIME-defined accessor on the base
//! (defineProperty after class definition) reads through the plain
//! member path with the prototype as receiver; a setter shadowed by
//! the subclass's own same-name setter resolves via `this` on the
//! fallback write path.
//!
//! Consumed marker nodes are overwritten with `Expr::Null` so no
//! orphan `__superbase__` survives in the arena for later
//! arena-scanning passes to trip over (rotation 333 blade 7 posture).

use super::desugar_classes_super::ClassIndexEntry;
use super::super_collect_prop::{SuperPropSite, SuperPropSites, collect_superprop_in_stmt};
use super::{AccessorKind, Ast, ClassMethod, Expr, ExprId};

/// What the first chain class declaring `name` says about it.
struct DeclaredShape {
    owner: String,
    has_getter: bool,
    has_setter: bool,
}

pub(super) fn rewrite_super_prop_sites(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    for (_, cname, _tp, parent, _, _, ctor, methods, static_methods) in class_index {
        let Some(parent_name) = parent.as_ref() else {
            continue;
        };
        // Instance context: ctor body (field inits are folded in) +
        // instance methods. Super base = <Parent>.prototype, receiver
        // spelling = `this` (Pass 2 rewrites it to `__this`).
        let mut inst = SuperPropSites::default();
        if let Some(c) = ctor.as_ref() {
            for s in &c.body {
                collect_superprop_in_stmt(ast, s, &mut inst);
            }
        }
        for m in methods {
            for s in &m.body {
                collect_superprop_in_stmt(ast, s, &mut inst);
            }
        }
        rewrite_sites(ast, class_index, parent_name, cname, false, inst);

        // Static context: super base = the <Parent> class object,
        // receiver spelling = the class's own name.
        let mut stat = SuperPropSites::default();
        for m in static_methods {
            for s in &m.body {
                collect_superprop_in_stmt(ast, s, &mut stat);
            }
        }
        rewrite_sites(ast, class_index, parent_name, cname, true, stat);
    }
}

fn rewrite_sites(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    parent_name: &str,
    cname: &str,
    is_static: bool,
    found: SuperPropSites,
) {
    for site in found.sites {
        match site {
            SuperPropSite::Read { member, name } => {
                let decl = nearest_declaring_shape(class_index, parent_name, &name, is_static);
                if let Some(d) = decl.filter(|d| d.has_getter) {
                    let callee =
                        ast.add_expr(Expr::Ident(accessor_fn(is_static, &d.owner, &name, "_get")));
                    let args = if is_static {
                        Vec::new()
                    } else {
                        vec![ast.add_expr(Expr::This)]
                    };
                    ast.exprs[member.0 as usize] = Expr::Call { callee, args };
                } else {
                    let base = mint_base(ast, parent_name, is_static);
                    ast.exprs[member.0 as usize] = Expr::Member { obj: base, name };
                }
            }
            SuperPropSite::IndexRead { index_expr } => {
                let Expr::Index { index, .. } = ast.get_expr(index_expr) else {
                    continue;
                };
                let index = *index;
                let base = mint_base(ast, parent_name, is_static);
                ast.exprs[index_expr.0 as usize] = Expr::Index { obj: base, index };
            }
            SuperPropSite::AssignName {
                assign,
                target,
                name,
                value,
                stmt_pos,
            } => {
                let decl = nearest_declaring_shape(class_index, parent_name, &name, is_static);
                if stmt_pos && decl.as_ref().is_some_and(|d| d.has_setter) {
                    let owner = decl.unwrap().owner;
                    let callee =
                        ast.add_expr(Expr::Ident(accessor_fn(is_static, &owner, &name, "_set")));
                    let args = if is_static {
                        vec![value]
                    } else {
                        let this = ast.add_expr(Expr::This);
                        vec![this, value]
                    };
                    ast.exprs[assign.0 as usize] = Expr::Call { callee, args };
                } else {
                    let recv = mint_receiver(ast, cname, is_static);
                    ast.exprs[target.0 as usize] = Expr::Member { obj: recv, name };
                }
            }
            SuperPropSite::AssignIndex { target_index } => {
                let Expr::Index { index, .. } = ast.get_expr(target_index) else {
                    continue;
                };
                let index = *index;
                let recv = mint_receiver(ast, cname, is_static);
                ast.exprs[target_index.0 as usize] = Expr::Index { obj: recv, index };
            }
        }
    }
    for m in found.markers {
        ast.exprs[m.0 as usize] = Expr::Null;
    }
}

fn accessor_fn(is_static: bool, owner: &str, name: &str, suffix: &str) -> String {
    if is_static {
        format!("__sm_{owner}__{name}{suffix}")
    } else {
        format!("__cm_{owner}__{name}{suffix}")
    }
}

fn mint_base(ast: &mut Ast, parent_name: &str, is_static: bool) -> ExprId {
    let parent = ast.add_expr(Expr::Ident(parent_name.to_string()));
    if is_static {
        parent
    } else {
        ast.add_expr(Expr::Member {
            obj: parent,
            name: "prototype".to_string(),
        })
    }
}

fn mint_receiver(ast: &mut Ast, cname: &str, is_static: bool) -> ExprId {
    if is_static {
        ast.add_expr(Expr::Ident(cname.to_string()))
    } else {
        ast.add_expr(Expr::This)
    }
}

/// Walk `start` and its ancestors for the first class declaring `name`
/// in the relevant member list (instance methods or statics), and
/// report whether that declaration carries a getter / setter face.
/// The FIRST declaring class decides — a plain method there shadows
/// any accessor further up, exactly like the runtime chain would.
fn nearest_declaring_shape(
    class_index: &[ClassIndexEntry],
    start: &str,
    name: &str,
    is_static: bool,
) -> Option<DeclaredShape> {
    let mut cur = start.to_string();
    for _ in 0..64 {
        let entry = class_index.iter().find(|e| e.1 == cur)?;
        let list: &[ClassMethod] = if is_static { &entry.8 } else { &entry.7 };
        let declared: Vec<&ClassMethod> = list.iter().filter(|m| m.name == name).collect();
        if !declared.is_empty() {
            return Some(DeclaredShape {
                owner: cur,
                has_getter: declared
                    .iter()
                    .any(|m| m.accessor_kind == Some(AccessorKind::Getter)),
                has_setter: declared
                    .iter()
                    .any(|m| m.accessor_kind == Some(AccessorKind::Setter)),
            });
        }
        cur = entry.3.clone()?;
    }
    None
}
