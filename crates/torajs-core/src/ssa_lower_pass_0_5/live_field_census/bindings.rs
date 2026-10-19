//! The binding half of the census: which parameters, locals and call
//! results hold a live cell at every read.
//!
//! A pointer-shaped slot can hold NULL or the undefined cell whatever
//! its static type says: a narrowing of `q: T | null` that a call since
//! the test undid hands the null to a `T` parameter, a local, a return.
//! The lowering guards such a read unless this census proves it live,
//! the same default the field census answers for fields.
//!
//! A parameter of a declared function is live when every call of the
//! function fills it with a live value and nothing writes it. That
//! needs every call to be visible: the function's name is used only as
//! the callee of a direct call (a value use reaches `call`, `apply`,
//! `Reflect.apply`, a callback), it is declared once and never
//! assigned, and its parameters are its own (no hidden receiver,
//! environment or `arguments` vector). Parameters are judged to a
//! greatest fixed point together with fields and functions, which is
//! sound for the same reason: a value that is not live enters through
//! a leaf shape at some call, and that call fails on its own.

use std::collections::HashMap as StdMap;

use crate::ast::{Ast, Expr, ExprId, Param};
use crate::check::Type;
use crate::ssa::id_hash::{HashMap, HashSet};
use crate::ssa_lower_nullable_guard::spells_undef_with_generic_cell;
use crate::ssa_lower_pass_0_5::arr_head_census::ExprMask;

use super::judge::{Visit, Visiting};
use super::scope::{Binder, Owner};
use super::walk::factory_class;
use super::{Live, THIS};

/// Direct call sites and value uses of declared functions.
#[derive(Default)]
pub(super) struct Calls<'a> {
    /// `f(..)` argument lists, by callee name.
    pub(super) args: HashMap<&'a str, Vec<&'a [ExprId]>>,
    /// Names some expression uses other than as a direct callee.
    pub(super) value_used: HashSet<&'a str>,
    /// A name of the program can be rebound by something the census
    /// does not see (`eval`, a property of `globalThis`), or an
    /// `arguments` vector aliases parameters: no binding is judged.
    pub(super) open: bool,
}

impl<'a> Calls<'a> {
    /// `fns` are the declared functions, the only names counted.
    pub(super) fn collect<T>(
        ast: &'a Ast,
        dead: &ExprMask,
        (retargets, fns): (&'a StdMap<ExprId, String>, &HashMap<&'a str, T>),
    ) -> Self {
        let mut c = Calls::default();
        let mut uses: HashMap<&str, usize> = HashMap::default();
        let mut callee_uses: HashMap<&str, usize> = HashMap::default();
        for (i, e) in ast.exprs.iter().enumerate() {
            if dead.contains(ExprId(i as u32)) {
                continue;
            }
            match e {
                Expr::Ident(n) => {
                    if matches!(n.as_str(), "eval" | "globalThis") || n.starts_with("__torajs_arg")
                    {
                        c.open = true;
                    }
                    if fns.contains_key(n.as_str()) {
                        *uses.entry(n).or_default() += 1;
                    }
                }
                Expr::Closure { fn_name, .. } => {
                    c.value_used.insert(fn_name);
                }
                // a function constructed is entered by no direct call
                Expr::New { class_name, .. } => {
                    c.value_used.insert(class_name);
                }
                Expr::Call { callee, args } => {
                    if let Expr::Ident(f) = ast.get_expr(*callee) {
                        let at = retargets.get(&ExprId(i as u32)).unwrap_or(f);
                        if fns.contains_key(f.as_str()) {
                            *callee_uses.entry(f).or_default() += 1;
                        }
                        if fns.contains_key(at.as_str()) {
                            c.args.entry(at).or_default().push(args);
                        }
                    }
                }
                _ => {}
            }
        }
        for (n, k) in uses {
            if callee_uses.get(n).copied().unwrap_or(0) < k {
                c.value_used.insert(n);
            }
        }
        c.retargeted_values(ast, dead, (retargets, fns));
        c
    }

    /// A generic function's value is the specialization the checker
    /// retargeted it to (the lowering takes that one's address).
    fn retargeted_values<T>(
        &mut self,
        ast: &Ast,
        dead: &ExprMask,
        (retargets, fns): (&'a StdMap<ExprId, String>, &HashMap<&'a str, T>),
    ) {
        for (e, m) in retargets {
            if matches!(ast.get_expr(*e), Expr::Ident(_))
                && !dead.contains(*e)
                && fns.contains_key(m.as_str())
            {
                self.value_used.insert(m);
            }
        }
    }
}

impl Live {
    /// `eid` reads a binding, or calls a declared function, a function
    /// value or a method, of a pointer-shaped type or a string, and the
    /// census did not prove the value live: the lowering guards a
    /// member read off it, and the throw analysis counts that guard as
    /// a raise.
    pub(crate) fn unproven_read(&self, eid: ExprId) -> bool {
        self.unproven.contains(eid)
    }

    /// The parameters of the function a direct call enters that the
    /// caller tests before the call.
    pub(crate) fn entry_params(&self, call: ExprId) -> &[usize] {
        self.entry.get(&call).map_or(&[], Vec::as_slice)
    }
}

/// A slot of this type spells a nullish value in band: a pointer, or a
/// string's NULL and immortal cells.
pub(super) fn holds_nullish_in_band(t: &Type) -> bool {
    *t == Type::String || spells_undef_with_generic_cell(t)
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

/// A declared function whose parameters are judged by its call sites:
/// one the program names, with no parameter the desugaring added.
pub(super) fn judged_fn(name: &str, params: &[Param]) -> bool {
    !name.starts_with("__") && params.iter().all(|p| !p.name.starts_with("__"))
}

impl<'a> super::Census<'a> {
    /// The `(function, index)` parameters judged by call sites.
    pub(super) fn judged_params(&self) -> HashSet<(&'a str, usize)> {
        if self.calls.open {
            return HashSet::default();
        }
        (self.fn_params.values().flatten())
            .filter(|(f, _)| !self.calls.value_used.contains(f) && !self.assigned.contains(f))
            .copied()
            .collect()
    }
}

impl<'a, 'b> super::judge::State<'a, 'b> {
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
}
