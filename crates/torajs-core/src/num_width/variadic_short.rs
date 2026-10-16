//! A call through a variadic fn face may hand a function stored there
//! fewer arguments than it declares.
//!
//! `const fs: Array<(...xs: number[]) => number> = []` admits
//! `(a: number, b: number) => a + b`, and `fs[0](1)` then binds `b`
//! to `undefined` (§10.2.11: a parameter no argument reaches is
//! `undefined`). The call rides the boxed adapter, which hands an
//! `undefined` that an integer slot cannot hold to the function's
//! all-`any` twin, and keeps it as the `undefined` sentinel in a
//! `number` slot. Either way the answer may be NaN, so the face's
//! return cannot stay on the integer lane the residents' typed bodies
//! would otherwise give it (`fs[0](1)` printed 0), and the parameters
//! no argument reaches join the sentinel-capable ones, so a read that
//! asks (`typeof b`, `b === undefined`) answers for `undefined`.
//!
//! A call whose arguments are unknown from some position on (a spread,
//! an `apply` array that is no literal) is recorded the same way, and
//! the parameters from there on are numbers of any width besides.
//!
//! The residents are only known once the union-find is complete, so
//! the walk records each such call and this runs after the container
//! keys are canonical, before the fixpoint.

use std::collections::HashSet;

use super::container::canon_key_frozen;
use super::{Analysis, SlotKey};
use crate::ast::{Expr, ExprId, PropKey};
use crate::check::Type;

impl Analysis<'_> {
    /// Record an indirect call whose callee is typed by a variadic fn
    /// face, with the first position its arguments may leave unfilled
    /// (a spread stands for an unknown count from its own position).
    pub(super) fn record_variadic_call(&mut self, callee: ExprId, args: &[ExprId], ck: &SlotKey) {
        let Some(Type::Function(params, _)) = self.expr_types.get(&callee) else {
            return;
        };
        if !matches!(params.last(), Some(Type::Rest(_))) {
            return;
        }
        let filled = args
            .iter()
            .position(|a| matches!(self.ast.get_expr(*a), Expr::Spread { .. }))
            .unwrap_or(args.len());
        self.variadic_calls.push((ck.clone(), filled, false));
    }

    /// Record an indirect call whose arguments from position `from` on
    /// are not known where it is written: a spread (`f(1, ...xs)`,
    /// `f.call(t, ...xs)`) or an `apply` array that is no literal
    /// (`f.apply(t, xs)`). Each parameter from there may take any
    /// number, or none.
    pub(super) fn record_unlisted_args(&mut self, ck: &SlotKey, from: usize) {
        self.variadic_calls.push((ck.clone(), from, true));
    }
}

/// Seed the `__ret` face of every recorded call that leaves a declared
/// parameter of some function stored in its callee's slot unfilled,
/// and name those parameters in `undef_params`; a call whose arguments
/// are unknown from there on seeds those parameters too.
pub(super) fn seed(a: &mut Analysis<'_>, undef_params: &mut HashSet<(String, String)>) {
    for (ck, filled, unlisted) in std::mem::take(&mut a.variadic_calls) {
        let slot = canon_key_frozen(&a.uf, &ck);
        let mut short = false;
        let mut wide = Vec::new();
        for (key, f, params) in &a.fn_flows {
            if params.len() > filled && canon_key_frozen(&a.uf, key) == slot {
                short = true;
                undef_params.extend(params[filled..].iter().map(|p| (f.clone(), p.clone())));
                if unlisted {
                    let pks = params[filled..]
                        .iter()
                        .map(|p| SlotKey::Param(f.clone(), p.clone()));
                    wide.extend(pks.map(|pk| canon_key_frozen(&a.uf, &pk)));
                }
            }
        }
        a.seeds.extend(wide);
        if short {
            let rk = SlotKey::Field(Box::new(ck), PropKey::from("__ret"));
            a.seeds.push(canon_key_frozen(&a.uf, &rk));
        }
    }
}
