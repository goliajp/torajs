//! What the fixed point answers about reads and calls, and the
//! judgements of the fixed point itself.

use crate::ast::{Expr, ExprId};
use crate::check::Type;
use crate::ssa::id_hash::{HashMap, HashSet};

use super::bindings::holds_nullish_in_band;
use super::judge::{State, Visit, Visiting};
use super::scope::{Binder, Owner};
use super::walk::factory_class;
use super::{Entry, ExprMask, THIS};

impl<'a, 'b> State<'a, 'b> {
    /// What the fixed point answers about reads: the unproven reads and
    /// calls, the member reads a discriminant proved, and the arguments
    /// each call tests for the callee.
    pub(super) fn reads(&self, entry: &HashMap<&str, Vec<usize>>) -> (ExprMask, ExprMask, Entry) {
        let open = self.c.calls.open;
        let fns = (self.live_fns.iter()).filter(|f| !open && self.call_live(f));
        let members = if open {
            ExprMask::default()
        } else {
            self.proven_members()
        };
        let unproven = self.unproven(&fns.copied().collect());
        (unproven, members, self.entry_calls(entry))
    }

    pub(super) fn field_live(&self, class: &str, field: &str) -> bool {
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

    pub(super) fn fn_live(&self, name: &str) -> bool {
        let Some(Some(rets)) = self.rets.get(name) else {
            return false;
        };
        if self.c.widths.returns_undef_on_fallthrough(name) || self.c.assigned.contains(name) {
            return false;
        }
        let mut visiting = Visiting::default();
        rets.iter().all(|e| self.value_live(*e, &mut visiting))
    }

    /// The reads and calls [`Live::unproven_read`] answers, given the
    /// declared functions whose calls are live. A method's receiver is
    /// left out, since a call through a nullish one raises at the call
    /// site; a name that can be rebound out of sight proves nothing.
    pub(super) fn unproven(&self, fns: &HashSet<&str>) -> ExprMask {
        let c = self.c;
        let open = c.calls.open;
        let binder = |n: &str| {
            c.let_inits.contains_key(n) || c.scopes.param_names.contains(n) || c.bound.contains(n)
        };
        let mut reads: HashMap<(Owner, &str), bool> = HashMap::default();
        let mut methods: HashMap<(&str, &str), bool> = HashMap::default();
        let mut out = ExprMask::new(c.ast);
        for (i, e) in c.ast.exprs.iter().enumerate() {
            let id = ExprId(i as u32);
            if !matches!(e, Expr::Ident(_) | Expr::Call { .. })
                || !c.ty(id).is_some_and(holds_nullish_in_band)
            {
                continue;
            }
            let unproven = match e {
                Expr::Ident(n) if matches!(n.as_str(), THIS | "__new_target") => false,
                Expr::Ident(n) => {
                    let owner = c.scopes.owner(id);
                    let mut live = || self.read_live(owner, n, &mut Visiting::default());
                    binder(n) && (open || !*reads.entry((owner, n)).or_insert_with(&mut live))
                }
                Expr::Call { callee, .. } => match c.ast.get_expr(*callee) {
                    Expr::Ident(f) if c.fns.contains_key(f.as_str()) => {
                        let f = c.retargets.get(&id).unwrap_or(f);
                        factory_class(f).is_none() && !fns.contains(f.as_str())
                    }
                    Expr::Ident(f) => binder(f),
                    Expr::Member { obj, name } => {
                        let mut live = |class| {
                            let m = methods.entry((class, name.as_str()));
                            *m.or_insert_with(|| self.methods_live(class, name))
                        };
                        match c.ty(*obj) {
                            Some(Type::ClassRef(_)) => {
                                open || !self.method_class(*obj).is_some_and(live)
                            }
                            Some(t) => open || recv_unproven(t, &mut live),
                            None => false,
                        }
                    }
                    _ => false,
                },
                _ => false,
            };
            if unproven {
                out.insert(id);
            }
        }
        out
    }

    /// Every direct call of `f` fills parameter `i` with a live value,
    /// and nothing writes the parameter.
    pub(super) fn param_live(&self, f: &str, i: usize) -> bool {
        let Some((params, _)) = self.c.fns.get(f) else {
            return false;
        };
        let Some(p) = params.get(i) else { return false };
        if p.is_rest || p.default.is_some() || self.c.assigned.contains(p.name.as_str()) {
            return false;
        }
        // a function no direct call enters is entered out of sight
        let Some(sites) = self.c.calls.args.get(f) else {
            return false;
        };
        let mut visiting = Visiting::default();
        sites.iter().all(|args| {
            args.get(i).is_some_and(|a| {
                (args.iter().take(i + 1))
                    .all(|b| !matches!(self.c.ast.get_expr(*b), Expr::Spread { .. }))
                    && self.value_live(*a, &mut visiting)
            })
        })
    }

    /// Every binder of `n` a read in `owner` can reach is live: a `let`
    /// by its initialiser and by every `n = v` store, a constructor
    /// parameter by the class's construction sites, a function
    /// parameter by the function's call sites, a function declaration
    /// by being one.
    pub(super) fn read_live(&self, owner: Owner, n: &'a str, visiting: &mut Visiting<'b>) -> bool {
        let c = self.c;
        let Some(reach) = c.scopes.reach(owner, n) else {
            return false;
        };
        if reach.open {
            return false;
        }
        let (params, lets) = (&reach.params, &reach.lets);
        let writes = c.assign_values.get(n).map_or(&[][..], Vec::as_slice);
        if params.is_empty() && lets.is_empty() {
            // a function declaration's name is its function object
            return (c.fns.contains_key(n) || reach.split) && !c.assigned.contains(n);
        }
        // a function redeclared under a binding's name
        if c.assigned.contains(n) && (writes.is_empty() || c.fns.contains_key(n)) {
            return false;
        }
        // a read met again on the way is judged by its other sources,
        // through one of which any value that is not live has to enter
        if !visiting.insert(Visit::Read(owner, n)) {
            return true;
        }
        let ok = (lets.iter().chain(writes)).all(|i| self.value_live(*i, visiting))
            && params.iter().all(|b| match b {
                Binder::Ctor(class, i) => self.ctor_param_live(class, *i, visiting),
                Binder::Param(f, i) => self.live_params.contains(&(*f, *i)),
                Binder::Open => false,
            });
        visiting.remove(&Visit::Read(owner, n));
        ok
    }

    /// The arguments each direct call of a function in `entry` tests:
    /// those at a tested index the census does not prove live there.
    pub(super) fn entry_calls(&self, entry: &HashMap<&str, Vec<usize>>) -> Entry {
        let c = self.c;
        let mut out = HashMap::default();
        if entry.is_empty() {
            return out;
        }
        for (i, e) in c.ast.exprs.iter().enumerate() {
            let id = ExprId(i as u32);
            if let Expr::Call { callee, args } = e
                && let Expr::Ident(f) = c.ast.get_expr(*callee)
                && let Some(at) = entry.get(c.retargets.get(&id).unwrap_or(f).as_str())
            {
                let test: Vec<usize> = (at.iter().copied())
                    .filter(|i| {
                        (args.get(*i))
                            .is_some_and(|a| !self.value_live(*a, &mut Visiting::default()))
                    })
                    .collect();
                if !test.is_empty() {
                    out.insert(id, test);
                }
            }
        }
        out
    }
}

/// A method called through a receiver of type `t` other than a named
/// class may answer a value the census did not prove live: a struct's
/// function field, a type parameter, or a union one of whose classes
/// `live` does not prove the method of. Any other receiver is a builtin
/// whose methods answer fresh values.
fn recv_unproven<'a>(t: &'a Type, live: &mut impl FnMut(&'a str) -> bool) -> bool {
    match t {
        Type::Struct(_) | Type::TypeVar(_) => true,
        Type::ClassRef(c) => !live(c.split('<').next().unwrap_or(c)),
        Type::Nullable(inner) => recv_unproven(inner, live),
        Type::Union(ts) => ts.iter().any(|t| recv_unproven(t, live)),
        _ => false,
    }
}
