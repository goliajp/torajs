//! Math.* / Date.<static> trailing-arg ignore wedge
//! extracted from [`crate::check_type_of_call::check`]'s
//! post-cascade generic-call mechanics (chunk 304 —
//! ninety-sixth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! S243 / S250 — Per ES §21.3.2.* / §21.4.3.*: each Math.*
//! / Date.<static> algorithm reads only the declared
//! positional args; the surface `Type::Function` sig is
//! closed but the underlying calling convention is
//! open-arity. SSA-emit's generic Math.* / Date.<static>
//! Call lowering already truncates extras before the
//! (f64, ...) / (intrinsic) helper ABI, so we only need
//! to truncate at the check arity gate and `type_of` the
//! trailing operands for side effects. Other builtin
//! receivers (Number/Array/String) stay on the existing
//! narrow carve-outs (S238–S248) until per-method SSA-emit
//! shape widens to match.
//!
//! Mutator-shape sibling: takes `effective_args: &mut
//! Vec<ExprId>` so the wedge can truncate the trailing
//! positions; `params` is read-only borrowed (the wedge
//! does not modify `params`).
//!
//! Returns:
//! - `Ok(())` — wedge applied (effective_args truncated
//!   to params.len()) or did not fire (effective_args.len()
//!   <= params.len(), callee not a Member, receiver not
//!   Math / Date namespace)
//! - `Err(_)` — `type_of` failed on receiver or any
//!   trailing operand

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn apply(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    effective_args: &mut Vec<ExprId>,
    params: &[Type],
) -> Result<(), String> {
    if effective_args.len() <= params.len() {
        return Ok(());
    }
    let Expr::Member { obj, name: _ } = ast.get_expr(*callee) else {
        return Ok(());
    };
    let recv_ty = checker.type_of(ast, *obj)?;
    if matches!(recv_ty, Type::Object("Math") | Type::Object("Date")) {
        for &aid in &effective_args[params.len()..] {
            let _ = checker.type_of(ast, aid)?;
        }
        effective_args.truncate(params.len());
    }
    Ok(())
}
