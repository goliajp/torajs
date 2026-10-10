//! The judgement half of the census: the fixed point over candidate
//! fields and functions, and what "live by construction" means for a
//! value, a constructor parameter and a function's returns.

use std::collections::{HashMap, HashSet};

use crate::ast::desugar_async_sm_walk::visit_stmt_exprs;
use crate::ast::{Expr, ExprId, Stmt};
use crate::check::Type;

use super::walk::{
    collect_super_args, derives_from, factory_class, own_returns, pointer_shaped, user_params,
};
use super::{Census, LiveFields, THIS};

impl<'a> Census<'a> {
    /// Greatest fixed point over the candidate fields and functions.
    pub(super) fn solve(&self) -> LiveFields {
        // `super(..)` inside a derived constructor constructs the parent
        let mut construct_args = self.construct_args.clone();
        for ctor in &self.ctors {
            let Some(Some(parent)) = self.ast.class_parents.get(&ctor.class) else {
                continue;
            };
            for s in ctor.body {
                visit_stmt_exprs(s, &mut |e| {
                    collect_super_args(self.ast, e, &mut |args| {
                        construct_args
                            .entry(parent.clone())
                            .or_default()
                            .push(args.to_vec());
                    });
                });
            }
        }
        let mut live: LiveFields = self
            .prefix
            .iter()
            .filter(|w| !self.foreign_classes.contains(&w.class))
            .filter(|w| self.field_type_live(&w.class, &w.field))
            .map(|w| (w.class.clone(), w.field.clone()))
            .collect();
        let mut live_fns: HashSet<String> = self.fns.keys().cloned().collect();
        loop {
            let st = State {
                c: self,
                live: &live,
                live_fns: &live_fns,
                construct_args: &construct_args,
            };
            let next_live: LiveFields = live
                .iter()
                .filter(|(class, field)| st.field_live(class, field))
                .cloned()
                .collect();
            let next_fns: HashSet<String> =
                live_fns.iter().filter(|f| st.fn_live(f)).cloned().collect();
            if next_live.len() == live.len() && next_fns.len() == live_fns.len() {
                break;
            }
            live = next_live;
            live_fns = next_fns;
        }
        // a derived class reads the field through its own class name
        let classes: Vec<&String> = self.ast.class_parents.keys().collect();
        let mut out = live.clone();
        for (class, field) in &live {
            for d in &classes {
                if derives_from(self.ast, d, class) && !self.foreign_classes.contains(*d) {
                    out.insert(((*d).clone(), field.clone()));
                }
            }
        }
        out
    }

    /// The field is declared on the class with a non-nullable
    /// pointer-shaped type.
    fn field_type_live(&self, class: &str, field: &str) -> bool {
        self.ast.stmts.iter().any(|s| {
            let Stmt::TypeDecl { name, fields, .. } = s else {
                return false;
            };
            name == class
                && fields.iter().any(|(k, ann)| {
                    k.lossy() == field
                        && !ann.starts_with("__nullable(")
                        && !matches!(ann.as_str(), "any" | "undefined" | "void" | "null")
                })
        }) && self.prefix.iter().any(|w| {
            w.class == class
                && w.field == field
                && self
                    .expr_types
                    .get(&w.value)
                    .is_some_and(|t| pointer_shaped(t))
        })
    }
}

struct State<'a, 'b> {
    c: &'b Census<'a>,
    live: &'b LiveFields,
    live_fns: &'b HashSet<String>,
    construct_args: &'b HashMap<String, Vec<Vec<ExprId>>>,
}

impl State<'_, '_> {
    fn field_live(&self, class: &str, field: &str) -> bool {
        if self.c.deleted.contains(field) || self.c.foreign.contains(field) {
            return false;
        }
        let prefix_ok = self
            .c
            .prefix
            .iter()
            .filter(|w| w.class == class && w.field == field)
            .all(|w| match w.param {
                Some(i) => self.ctor_param_live(class, i, &mut HashSet::new()),
                None => self.value_live(w.value, &mut HashSet::new()),
            });
        prefix_ok
            && self
                .c
                .member_writes
                .get(field)
                .is_none_or(|ws| ws.iter().all(|v| self.value_live(*v, &mut HashSet::new())))
    }

    /// Every construction of `class` fills user parameter `i` with a
    /// live value, and the constructor never reassigns it.
    fn ctor_param_live(&self, class: &str, i: usize, visiting: &mut HashSet<String>) -> bool {
        let Some(ctor) = self.c.ctors.iter().find(|c| c.class == class) else {
            return false;
        };
        let user = user_params(ctor.params);
        let Some(p) = user.get(i) else { return false };
        if p.is_rest || p.default.is_some() || self.c.assigned.contains(&p.name) {
            return false;
        }
        let Some(sites) = self.construct_args.get(class) else {
            return false;
        };
        // a site inside the constructor passing its own parameter on
        // resolves to the other sites
        let key = format!("{class}#{i}");
        if !visiting.insert(key.clone()) {
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
        visiting.remove(&key);
        ok
    }

    fn fn_live(&self, name: &str) -> bool {
        let Some((_, body)) = self.c.fns.get(name) else {
            return false;
        };
        if self.c.widths.returns_undef_on_fallthrough(name) || self.c.assigned.contains(name) {
            return false;
        }
        let mut ok = true;
        own_returns(body, &mut |r| {
            ok = ok
                && match r {
                    Some(e) => self.value_live(e, &mut HashSet::new()),
                    None => false,
                }
        });
        ok
    }

    fn value_live(&self, e: ExprId, visiting: &mut HashSet<String>) -> bool {
        let ast = self.c.ast;
        if self
            .c
            .expr_types
            .get(&e)
            .is_some_and(|t| !pointer_shaped(t))
        {
            return false;
        }
        match ast.get_expr(e) {
            Expr::New { .. }
            | Expr::Array(_)
            | Expr::ObjectLit { .. }
            | Expr::ArrowFn { .. }
            | Expr::Closure { .. }
            | Expr::Regex { .. } => true,
            // a method's receiver is whatever its caller passed
            Expr::This => self.c.ctor_exprs.contains(&e),
            Expr::Ident(n) if n == THIS => self.c.ctor_exprs.contains(&e),
            // every binder of the name must be live: a `let` by its
            // initialiser, a constructor parameter by the class's
            // construction sites, anything else by nothing
            Expr::Ident(n) => {
                if self.c.bound.contains(n)
                    || self.c.assigned.contains(n)
                    || !visiting.insert(n.clone())
                {
                    return false;
                }
                let lets = self.c.let_inits.get(n).map_or(&[][..], Vec::as_slice);
                let params = self.c.ctor_params.get(n).map_or(&[][..], Vec::as_slice);
                let ok = !(lets.is_empty() && params.is_empty())
                    && lets.iter().all(|i| self.value_live(*i, visiting))
                    && params
                        .iter()
                        .all(|(class, i)| self.ctor_param_live(class, *i, visiting));
                visiting.remove(n);
                ok
            }
            Expr::As { expr, .. } => self.value_live(*expr, visiting),
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => self.value_live(*then_branch, visiting) && self.value_live(*else_branch, visiting),
            Expr::Call { callee, .. } => match ast.get_expr(*callee) {
                Expr::Ident(f) => {
                    factory_class(f).is_some() || (self.live_fns.contains(f) && !self.c.shadowed(f))
                }
                _ => false,
            },
            Expr::Member { obj, name } => match self.c.expr_types.get(obj) {
                Some(Type::ClassRef(c)) => {
                    let c = c.split('<').next().unwrap_or(c);
                    self.live.contains(&(c.to_string(), name.clone()))
                }
                _ => false,
            },
            _ => false,
        }
    }
}
