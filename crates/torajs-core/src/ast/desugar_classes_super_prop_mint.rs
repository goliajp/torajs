//! The per-site context of the SuperProperty rewrite
//! (`desugar_classes_super_prop`) and the mints every arm shares:
//! the super base, the receiver, the accessor face name, the update
//! kernel's spelling and the chain's declared shape of a name. Split
//! out under the 500-line file rule when the update / optional-call
//! arms landed (rotation 628); bodies verbatim.

use super::desugar_classes_super::ClassIndexEntry;
use super::{AccessorKind, Ast, ClassMethod, Expr, ExprId};

/// What every site of one class context shares: whose base, whose
/// receiver, static or instance, and whether the base is read at
/// runtime off the home object (`dynamic`).
pub(super) struct SiteCtx<'a> {
    pub(super) parent_name: Option<&'a str>,
    pub(super) cname: &'a str,
    pub(super) is_static: bool,
    pub(super) dynamic: bool,
}

impl SiteCtx<'_> {
    /// The super base — minted fresh per site.
    pub(super) fn base(&self, ast: &mut Ast) -> ExprId {
        // 507-03 — the parent is only the home object's prototype while
        // nothing re-links it; when the program can, the base is read from
        // the home object itself. That also covers the no-extends case
        // uniformly: `Object.getPrototypeOf(C.prototype)` IS
        // %Object.prototype% there.
        if self.dynamic {
            return super::super_home_dynamic::mint_home_proto(ast, self.cname, self.is_static);
        }
        // No extends clause → the instance super base is %Object.prototype%
        // and the static one %Function.prototype% (both read as the
        // builtin prototype singleton off the namespace).
        match (self.parent_name, self.is_static) {
            (Some(p), true) => ast.add_expr(Expr::Ident(p.to_string())),
            (Some(p), false) => {
                let parent = ast.add_expr(Expr::Ident(p.to_string()));
                ast.add_expr(Expr::Member {
                    obj: parent,
                    name: "prototype".to_string(),
                })
            }
            (None, is_static) => {
                let ns = ast.add_expr(Expr::Ident(
                    if is_static { "Function" } else { "Object" }.to_string(),
                ));
                ast.add_expr(Expr::Member {
                    obj: ns,
                    name: "prototype".to_string(),
                })
            }
        }
    }

    /// §13.3.7's receiver — `this`, or the class object in static
    /// context.
    pub(super) fn receiver(&self, ast: &mut Ast) -> ExprId {
        if self.is_static {
            ast.add_expr(Expr::Ident(self.cname.to_string()))
        } else {
            ast.add_expr(Expr::This)
        }
    }
}

/// What the first chain class declaring `name` says about it.
pub(super) struct DeclaredShape {
    pub(super) owner: String,
    pub(super) has_getter: bool,
    pub(super) has_setter: bool,
}

pub(super) fn accessor_fn(is_static: bool, owner: &str, name: &str, suffix: &str) -> String {
    if is_static {
        format!("__sm_{owner}__{name}{suffix}")
    } else {
        format!("__cm_{owner}__{name}{suffix}")
    }
}

/// The update kernel's four spellings — the flags ride the callee
/// name so the call lowering appends them as constants.
pub(super) fn update_fn(is_inc: bool, is_prefix: bool) -> &'static str {
    match (is_inc, is_prefix) {
        (true, false) => "__torajs_super_prop_incr_post",
        (true, true) => "__torajs_super_prop_incr_pre",
        (false, false) => "__torajs_super_prop_decr_post",
        (false, true) => "__torajs_super_prop_decr_pre",
    }
}

/// Walk `start` and its ancestors for the first class declaring `name`
/// in the relevant member list (instance methods or statics), and
/// report whether that declaration carries a getter / setter face.
/// The FIRST declaring class decides — a plain method there shadows
/// any accessor further up, exactly like the runtime chain would.
pub(super) fn nearest_declaring_shape(
    class_index: &[ClassIndexEntry],
    start: Option<&str>,
    name: &str,
    is_static: bool,
) -> Option<DeclaredShape> {
    let mut cur = start?.to_string();
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
