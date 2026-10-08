//! The direct calls of `hidden_any`: what an argument carries in, and
//! what a call carries out.
//!
//! `id(d[1])` against `function id(v: number)` converted an element of
//! an escaped typed array to a number on the way into the typed slot,
//! where §10.2.1.3 binds the argument as passed. An argument that may
//! carry such a value is reported to the check driver, which checks the
//! program again with the call retargeted to a clone taking that
//! parameter as `any` (`check_type_of_call::any_widen`). The clone
//! answers `any` when its result depends on the parameter, and the
//! value it answers flows on as a source.
//!
//! A function returning such a value (`function get(): number { return
//! d[1] }`) converted it to its declared return on the way out, where
//! §10.2.1.12 hands the value back as is. The check driver retypes its
//! return `any`, and a call of it carries what it returns.

use super::Analysis;
use super::Scope;
use super::hidden_any::{Graph, Node};
use crate::ast::{Expr, ExprId};
use crate::check::Type;

impl Graph<'_> {
    /// Edges from the sources each argument of a direct call carries
    /// into that argument's node. Only a scalar or `any` argument can
    /// meet a scalar parameter.
    pub(super) fn call_args(
        &mut self,
        a: &Analysis<'_>,
        callee: ExprId,
        args: &[ExprId],
        sc: &Scope,
    ) {
        // a relay shell's forwarding call hands its target what the
        // shell's typed face took; the shell's own twin takes the rest
        if !matches!(a.ast.get_expr(callee), Expr::Ident(_)) || crate::ast::is_shell(sc.fn_name) {
            return;
        }
        for &arg in args {
            if matches!(
                a.expr_types.get(&arg),
                Some(Type::Number | Type::String | Type::Boolean | Type::Any)
            ) {
                self.flow(a, &[Node::Arg(arg)], arg, sc);
            }
        }
    }
}

impl Graph<'_> {
    /// What call `eid` of `callee` returns, when `callee` names a
    /// function declaration (or the instance a retarget names).
    pub(super) fn call_result(
        &self,
        a: &Analysis<'_>,
        eid: ExprId,
        callee: ExprId,
    ) -> Option<Node> {
        let Expr::Ident(n) = a.ast.get_expr(callee) else {
            return None;
        };
        if self.frames.iter().any(|f| f.contains_key(n)) || a.shadowing_reads.contains(&callee) {
            return None;
        }
        let f = a.retargets.get(&eid).unwrap_or(n);
        a.fn_params.contains_key(f).then(|| Node::Ret(f.clone()))
    }
}

/// Does call `eid` answer through an any-widened clone typed `any`?
pub(super) fn answers_any(a: &Analysis<'_>, eid: ExprId) -> bool {
    a.retargets.get(&eid).is_some_and(|f| f.contains("$$anyw"))
        && a.expr_types.get(&eid) == Some(&Type::Any)
}
