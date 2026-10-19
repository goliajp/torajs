//! The judgement half of the census: the fixed point over candidate
//! fields and functions, and what "live by construction" means for a
//! value, a constructor parameter and a function's returns.

use crate::ssa::id_hash::{HashMap, HashSet};

use crate::ast::desugar_async_sm_walk::visit_stmt_exprs;
use crate::ast::{Expr, ExprId, Stmt};
use crate::check::Type;

use super::disc::Discs;
use super::scope::Owner;
use super::walk::{collect_super_args, derives_from, pointer_shaped};
use super::{Census, ExprMask, Live, LiveFields, THIS};

/// A read or a constructor parameter met again on the way, judged by
/// its other sources.
#[derive(PartialEq, Eq, Hash)]
pub(super) enum Visit<'b> {
    Read(Owner, &'b str),
    Ctor(usize, usize),
}

pub(super) type Visiting<'b> = HashSet<Visit<'b>>;

impl<'a> Census<'a> {
    /// Greatest fixed point over the candidate fields, functions and
    /// parameters; `fields_open` says no road the census cannot see
    /// reaches a field.
    pub(super) fn solve(&self, fields_open: bool) -> Live {
        let construct_args = self.construct_sites();
        let mut live: HashSet<(&str, &str)> = if fields_open {
            (self.prefix.iter())
                .filter(|w| !self.foreign_classes.contains(w.class))
                .filter(|w| self.field_type_live(w.class, w.field))
                .map(|w| (w.class, w.field))
                .collect()
        } else {
            HashSet::default()
        };
        let mut live_fns: HashSet<&str> = self.fns.keys().copied().collect();
        let judged = self.judged_params();
        let mut entry = self.entry_checked(&judged);
        let checked: HashSet<(&str, usize)> = (entry.iter())
            .flat_map(|(f, at)| at.iter().map(|i| (*f, *i)))
            .collect();
        let mut live_params = judged;
        // what the rounds read again and again, gathered once
        let (mut discs, methods, rets) = (self.discs(), self.methods(), self.returns());
        self.disc_plans(&mut discs, &construct_args, fields_open);
        let (unproven, members) = loop {
            let st = State {
                c: self,
                live: &live,
                live_fns: &live_fns,
                live_params: &live_params,
                construct_args: &construct_args,
                discs: &discs,
                methods: &methods,
                rets: &rets,
                fields_open,
            };
            let next_live: HashSet<(&str, &str)> = (live.iter())
                .filter(|(c, f)| st.field_live(c, f))
                .copied()
                .collect();
            let next_fns: HashSet<&str> =
                live_fns.iter().filter(|f| st.fn_live(f)).copied().collect();
            let next_params: HashSet<(&str, usize)> = (live_params.iter())
                .filter(|p| checked.contains(*p) || st.param_live(p.0, p.1))
                .copied()
                .collect();
            if next_live.len() == live.len()
                && next_fns.len() == live_fns.len()
                && next_params.len() == live_params.len()
            {
                let open = self.calls.open;
                let fns = (live_fns.iter()).filter(|f| !open && st.call_live(f));
                // a parameter every call fills with a live value needs
                // no test at the call either
                for (f, at) in entry.iter_mut() {
                    at.retain(|i| !st.param_live(f, *i));
                }
                let members = if open {
                    ExprMask::default()
                } else {
                    st.proven_members()
                };
                break (st.unproven(&fns.copied().collect()), members);
            }
            live = next_live;
            live_fns = next_fns;
            live_params = next_params;
        };
        // a derived class reads the field through its own class name
        let mut fields: LiveFields = (live.iter())
            .map(|(c, f)| (c.to_string(), f.to_string()))
            .collect();
        for (class, field) in &live {
            for d in self.ast.class_parents.keys() {
                if derives_from(self.ast, d, class) && !self.foreign_classes.contains(d.as_str()) {
                    fields.insert((d.clone(), field.to_string()));
                }
            }
        }
        let entry = self.entry_calls(&entry);
        Live {
            fields,
            unproven,
            members,
            entry,
        }
    }

    /// `new C(..)` and `super(..)` argument lists, by class: `super(..)`
    /// inside a derived constructor constructs the parent.
    fn construct_sites(&self) -> HashMap<&'a str, Vec<&'a [ExprId]>> {
        let mut out = self.construct_args.clone();
        for ctor in &self.ctors {
            let Some(Some(parent)) = self.ast.class_parents.get(ctor.class) else {
                continue;
            };
            for s in ctor.body {
                visit_stmt_exprs(s, &mut |e| {
                    collect_super_args(self.ast, e, &mut |args| {
                        out.entry(parent.as_str()).or_default().push(args);
                    });
                });
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
            w.class == class && w.field == field && self.ty(w.value).is_some_and(pointer_shaped)
        })
    }
}

pub(super) struct State<'a, 'b> {
    pub(super) c: &'b Census<'a>,
    live: &'b HashSet<(&'a str, &'a str)>,
    pub(super) live_fns: &'b HashSet<&'a str>,
    pub(super) live_params: &'b HashSet<(&'a str, usize)>,
    pub(super) construct_args: &'b HashMap<&'a str, Vec<&'a [ExprId]>>,
    pub(super) discs: &'b Discs<'a>,
    pub(super) methods: &'b super::fresh::Methods<'b>,
    rets: &'b HashMap<&'a str, Option<Vec<ExprId>>>,
    /// No road the census cannot see reaches a field or a method.
    pub(super) fields_open: bool,
}

impl<'b> State<'_, 'b> {
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
                Some(i) => self.ctor_param_live(class, i, &mut Visiting::default()),
                None => self.value_live(w.value, &mut Visiting::default()),
            });
        prefix_ok
            && self.c.member_writes.get(field).is_none_or(|ws| {
                ws.iter()
                    .all(|v| self.value_live(*v, &mut Visiting::default()))
            })
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
        let Some(sites) = self.construct_args.get(class) else {
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

    fn fn_live(&self, name: &str) -> bool {
        let Some(Some(rets)) = self.rets.get(name) else {
            return false;
        };
        if self.c.widths.returns_undef_on_fallthrough(name) || self.c.assigned.contains(name) {
            return false;
        }
        let mut visiting = Visiting::default();
        rets.iter().all(|e| self.value_live(*e, &mut visiting))
    }

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
}
