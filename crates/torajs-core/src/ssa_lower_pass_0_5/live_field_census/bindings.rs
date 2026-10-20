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

use super::Live;
use super::walk::{Few, NameFilter};

/// Direct call sites and value uses of declared functions.
#[derive(Default)]
pub(super) struct Calls<'a> {
    /// `f(..)` argument lists, by callee name.
    pub(super) args: HashMap<&'a str, Few<&'a [ExprId]>>,
    /// Names some expression uses other than as a direct callee.
    pub(super) value_used: HashSet<&'a str>,
    /// A name of the program can be rebound by something the census
    /// does not see (`eval`, a property of `globalThis`), or an
    /// `arguments` vector aliases parameters: no binding is judged.
    pub(super) open: bool,
}

/// [`Calls`] while the census's pass over the expressions is under
/// way: a name's value uses are known only once every expression is.
pub(super) struct CallsTally<'a> {
    calls: Calls<'a>,
    /// The declared functions' names, to skip the lookup of others.
    fns: NameFilter,
    /// Each declared function's uses as a name, less its uses as the
    /// callee of a direct call (every such callee is one of the uses).
    value_uses: HashMap<&'a str, isize>,
}

impl<'a> CallsTally<'a> {
    pub(super) fn new<T>(fns: &HashMap<&'a str, T>) -> Self {
        CallsTally {
            calls: Calls::default(),
            fns: NameFilter::of(fns.keys().copied()),
            value_uses: HashMap::default(),
        }
    }

    /// A live read of the name `n`; `fns` are the declared functions,
    /// the only names counted.
    pub(super) fn ident<T>(&mut self, n: &'a str, fns: &HashMap<&'a str, T>) {
        if matches!(n, "eval" | "globalThis") || n.starts_with("__torajs_arg") {
            self.calls.open = true;
        }
        if self.fns.may_hold(n) && fns.contains_key(n) {
            *self.value_uses.entry(n).or_default() += 1;
        }
    }

    /// A live closure of `n`, or a `new` of the class `n`: a function
    /// constructed is entered by no direct call.
    pub(super) fn value_use(&mut self, n: &'a str) {
        self.calls.value_used.insert(n);
    }

    /// The live call `id` of the name `f` with `args`.
    pub(super) fn call<T>(
        &mut self,
        (id, f, args): (ExprId, &'a str, &'a [ExprId]),
        (retargets, fns): (&'a StdMap<ExprId, String>, &HashMap<&'a str, T>),
    ) {
        if self.fns.may_hold(f) && fns.contains_key(f) {
            *self.value_uses.entry(f).or_default() -= 1;
        }
        let at = match retargets.is_empty() {
            true => f,
            false => retargets.get(&id).map_or(f, String::as_str),
        };
        if self.fns.may_hold(at) && fns.contains_key(at) {
            self.calls.args.entry(at).or_default().push(args);
        }
    }

    pub(super) fn finish<T>(
        self,
        ast: &'a Ast,
        dead: &ExprMask,
        (retargets, fns): (&'a StdMap<ExprId, String>, &HashMap<&'a str, T>),
    ) -> Calls<'a> {
        let mut c = self.calls;
        let used = (self.value_uses.into_iter()).filter_map(|(n, k)| (k > 0).then_some(n));
        c.value_used.extend(used);
        c.retargeted_values(ast, dead, (retargets, fns));
        c
    }
}

impl<'a> Calls<'a> {
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
    matches!(t, Type::String) || spells_undef_with_generic_cell(t)
}

/// A declared function whose parameters are judged by its call sites:
/// one the program names, with no parameter the desugaring added.
pub(super) fn judged_fn(name: &str, params: &[Param]) -> bool {
    !name.starts_with("__") && params.iter().all(|p| !p.name.starts_with("__"))
}
