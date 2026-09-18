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
    /// The static receiver is spelled `this` rather than the class
    /// binding — the ES5 capturing lane binds a static body's `this`
    /// to the class object (`.call(K)` / the defineProperty install)
    /// and rewrites the binding's own name inside its bodies, so the
    /// name would not survive there while `this` does.
    pub(super) this_receiver: bool,
}

impl<'a> SiteCtx<'a> {
    /// The context of a `super.m()` site whose base is read when it
    /// runs (a program that re-links prototypes): the same base and
    /// receiver spelling the property forms take, a unit class's
    /// included.
    pub(super) fn runtime(cname: &'a str, is_static: bool) -> Self {
        SiteCtx {
            parent_name: None,
            cname,
            is_static,
            dynamic: true,
            this_receiver: false,
        }
    }

    /// The super base — minted fresh per site.
    pub(super) fn base(&self, ast: &mut Ast) -> ExprId {
        // K5-3 — a unit class's bodies are hoisted away from the
        // evaluation that owns them, where neither its name nor its
        // parent's reads THAT evaluation's class object. The base is
        // read off the home the running `this` stands under, which is
        // also what §13.3.7 says (`[[HomeObject]].[[Prototype]]`).
        if ast.eval_unit_classes.contains(self.cname) {
            let callee = ast.add_expr(Expr::Ident(super::class_unit::UNIT_HOME.to_string()));
            let class = ast.add_expr(Expr::String(self.cname.into()));
            let this = self.running_this(ast);
            let home = ast.add_expr(Expr::Call {
                callee,
                args: vec![class, this],
            });
            return super::super_home_dynamic::home_proto_of(ast, home, self.is_static);
        }
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
        if ast.eval_unit_classes.contains(self.cname) {
            self.running_this(ast)
        } else if self.is_static && !self.this_receiver {
            ast.add_expr(Expr::Ident(self.cname.to_string()))
        } else {
            ast.add_expr(Expr::This)
        }
    }

    /// A unit class's `this`: in static context the parser's static
    /// `this`, which pass 2 turns into the evaluation's `__unit`.
    fn running_this(&self, ast: &mut Ast) -> ExprId {
        let this = ast.add_expr(Expr::This);
        if self.is_static {
            ast.static_this_sites.insert(this, self.cname.to_string());
        }
        this
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
