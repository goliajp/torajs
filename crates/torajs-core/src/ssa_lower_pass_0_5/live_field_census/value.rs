//! What "live by construction" means for a value: an expression, a
//! constructor parameter, a builtin's answer, a call.

use crate::ast::{BinOp, Expr, ExprId};
use crate::check::Type;

use super::THIS;
use super::judge::{State, Visit, Visiting};
use super::walk::{Few, derives_from, pointer_shaped};

impl<'a, 'b> State<'a, 'b> {
    pub(super) fn value_live(&self, e: ExprId, visiting: &mut Visiting<'b>) -> bool {
        let ast = self.c.ast;
        let ty = self.c.ty(e);
        if ty == Some(&Type::String) || (ty.is_none() && self.c.concat(e)) {
            return self.string_live(e, visiting);
        }
        // a nullable field read a discriminant test proved live
        if matches!(ty, Some(Type::Nullable(_))) && self.disc_live(e, visiting) {
            return true;
        }
        if ty.is_some_and(|t| !pointer_shaped(t)) {
            return false;
        }
        match ast.get_expr(e) {
            Expr::New { .. }
            | Expr::BigInt { .. }
            | Expr::Array(_)
            | Expr::ObjectLit { .. }
            | Expr::ArrowFn { .. }
            | Expr::Closure { .. }
            | Expr::Regex { .. } => true,
            // a method's receiver is whatever its caller passed
            Expr::This => self.c.ctor_exprs.contains(e),
            Expr::Ident(n) if n == THIS => self.c.ctor_exprs.contains(e),
            // every binder of the name must be live: a `let` by its
            // initialiser, a constructor parameter by the class's
            // construction sites, anything else by nothing
            Expr::Ident(n) => self.read_live(self.c.scopes.owner(e), n, visiting),
            Expr::As { expr, .. } => self.value_live(*expr, visiting),
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => self.value_live(*then_branch, visiting) && self.value_live(*else_branch, visiting),
            Expr::Call { callee, .. } => match ast.get_expr(*callee) {
                Expr::Ident(f) => self.call_live(self.c.retargets.get(&e).unwrap_or(f)),
                Expr::Member { obj, name } => {
                    self.c.fresh_builtin(*callee) || self.method_live(*obj, name)
                }
                _ => false,
            },
            Expr::Member { obj, name } => match self.c.ty(*obj) {
                Some(Type::ClassRef(c)) => {
                    let c = c.split('<').next().unwrap_or(c);
                    self.live.contains(&(c, name.as_str())) || self.disc_live(e, visiting)
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Every construction of `class` fills user parameter `i` with a
    /// live value, and the constructor never reassigns it.
    pub(super) fn ctor_param_live(
        &self,
        class: &str,
        i: usize,
        visiting: &mut Visiting<'b>,
    ) -> bool {
        let Some(at) = self.c.ctors.iter().position(|c| c.class == class) else {
            return false;
        };
        let ctor = &self.c.ctors[at];
        let user = &ctor.user;
        let Some(p) = user.get(i) else { return false };
        if p.is_rest || p.default.is_some() || self.c.assigned.contains(p.name.as_str()) {
            return false;
        }
        let Some(sites) = self.construct_args.get(class).map(Few::as_slice) else {
            return false;
        };
        // a site inside the constructor passing its own parameter on
        // resolves to the other sites
        if !visiting.insert(Visit::Ctor(at, i)) {
            return true;
        }
        let ok = sites.iter().all(|args| {
            args.get(i).is_some_and(|a| {
                !matches!(self.c.ast.get_expr(*a), Expr::Spread { .. })
                    && args
                        .iter()
                        .take(i)
                        .all(|b| !matches!(self.c.ast.get_expr(*b), Expr::Spread { .. }))
                    && self.value_live(*a, visiting)
            })
        });
        visiting.remove(&Visit::Ctor(at, i));
        ok
    }

    /// A string-typed value is never nullish: a literal, a `+` that
    /// concatenates, a `typeof`, a string builtin's answer, or a read
    /// or call judged as any other value is.
    pub(super) fn string_live(&self, e: ExprId, visiting: &mut Visiting<'b>) -> bool {
        match self.c.ast.get_expr(e) {
            Expr::String(_) | Expr::TypeOf { .. } => true,
            // a string element read the lowering tests where it is read,
            // and trusts once bound, as it did before this census
            Expr::Index { .. } => true,
            Expr::BinOp { op: BinOp::Add, .. } => true,
            Expr::Ident(n) => self.read_live(self.c.scopes.owner(e), n, visiting),
            Expr::As { expr, .. } => {
                self.c.ty(*expr) == Some(&Type::String) && self.value_live(*expr, visiting)
            }
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => self.value_live(*then_branch, visiting) && self.value_live(*else_branch, visiting),
            Expr::Call { callee, .. } => match self.c.ast.get_expr(*callee) {
                Expr::Ident(f) if self.c.fns.contains_key(f.as_str()) => {
                    self.call_live(self.c.retargets.get(&e).unwrap_or(f))
                }
                _ => self.c.string_builtin(*callee),
            },
            _ => false,
        }
    }

    /// A call of the declared function `f` answers a live value: a
    /// factory, a function all of whose returns are live, or a method
    /// dispatch stub every owner's body of which is.
    pub(super) fn call_live(&self, f: &str) -> bool {
        // the runtime's PromiseResolve answers a promise
        if super::walk::factory_class(f).is_some() || f == "__torajs_await_resolve" {
            return true;
        }
        if self.c.shadowed(f) {
            return false;
        }
        // a global called as a function answers a value of its kind
        if !self.c.fns.contains_key(f) && !self.c.assigned.contains(f) {
            return matches!(f, "Symbol" | "BigInt" | "Array" | "Object" | "RegExp");
        }
        let Some(m) = f.strip_prefix("__dispatch_") else {
            return self.live_fns.contains(f);
        };
        // the stub forwards to one owner; the slot reaches them all
        let (bare, suffix) = match m.split_once("$$") {
            Some((b, _)) => (b, &m[b.len()..]),
            None => (m, ""),
        };
        let owners = self.c.ast.method_owners.get(bare);
        owners.is_some_and(|os| {
            !os.is_empty()
                && os.iter().all(|o| {
                    let bodies = [
                        format!("__cm_{o}__{bare}{suffix}"),
                        format!("__cm_{o}__{bare}"),
                    ];
                    let mut found = bodies
                        .iter()
                        .filter(|b| self.c.fns.contains_key(b.as_str()));
                    let first = found.next();
                    first.is_some_and(|b| self.live_fns.contains(b.as_str()))
                        && found.all(|b| self.live_fns.contains(b.as_str()))
                })
        })
    }

    /// A call of method `m` off `obj`, a class instance, answers a
    /// live value: every body of the method the receiver's class, an
    /// ancestor or a subclass declares is live, and nothing else can
    /// stand in the slot (a property store, a field of the name).
    pub(super) fn method_live(&self, obj: ExprId, m: &str) -> bool {
        self.method_class(obj)
            .is_some_and(|class| self.methods_live(class, m))
    }

    /// The class of an instance receiver; none for a receiver that
    /// names a class (a static call).
    pub(super) fn method_class(&self, obj: ExprId) -> Option<&'b str> {
        let c = self.c;
        let Some(Type::ClassRef(class)) = c.ty(obj) else {
            return None;
        };
        let names_class =
            matches!(c.ast.get_expr(obj), Expr::Ident(n) if c.ast.class_parents.contains_key(n));
        (!names_class).then(|| class.split('<').next().unwrap_or(class))
    }

    /// Every body of `m` the class, an ancestor or a subclass declares
    /// is live, and nothing else can stand in the slot.
    pub(super) fn methods_live(&self, class: &str, m: &str) -> bool {
        let c = self.c;
        if !self.fields_open
            || c.foreign_classes.contains(class)
            || c.member_writes.contains_key(m)
            || c.foreign.contains(m)
            || c.deleted.contains(m)
            || c.prefix.iter().any(|w| w.field == m)
        {
            return false;
        }
        let mut found = false;
        for (o, f) in self.methods.get(m).into_iter().flatten() {
            if *o == class || derives_from(c.ast, o, class) || derives_from(c.ast, class, o) {
                if !self.live_fns.contains(*f) {
                    return false;
                }
                found = true;
            }
        }
        found
    }
}
