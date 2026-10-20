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

/// A declared function whose parameters are judged by its call sites:
/// one the program names, with no parameter the desugaring added.
pub(super) fn judged_fn(name: &str, params: &[Param]) -> bool {
    !name.starts_with("__") && params.iter().all(|p| !p.name.starts_with("__"))
}
