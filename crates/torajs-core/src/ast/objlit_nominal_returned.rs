//! r380 — the RETURNED-literal leg of the detached-objlit-method
//! family, split out of `objlit_nominal_anylane.rs` (adding it in
//! place pushed that file past the 500-line hard limit).
//!
//! Its sibling handles `const o = { m(){ this… } }; const t = o.m`,
//! where a binding carries the literal and can be annotated. Here
//! the receiver is a call result — `makeCounter().read` — so there
//! is nothing to annotate and the forcing goes on the `return`
//! itself.

use super::objlit_nominal_anylane::callee_positions;
use super::{Expr, ExprId, Stmt};

/// r380 — `(callee fn name, member)` pairs read as a value off a
/// direct call's RESULT (`makeCounter().read`). The twin of
/// [`value_read_members`] for the receiver shape that has no binding
/// to widen.
pub(super) fn value_read_call_members(
    exprs: &[Expr],
) -> std::collections::HashSet<(String, String)> {
    let callees = callee_positions(exprs);
    exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Expr::Member { obj, name } if !callees.contains(&(i as u32)) => {
                let Expr::Call { callee, .. } = &exprs[obj.0 as usize] else {
                    return None;
                };
                match &exprs[callee.0 as usize] {
                    Expr::Ident(f) => Some((f.clone(), name.clone())),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// r380 — the returned-literal leg of
/// [`widen_detached_method_objlits`]. `makeCounter().read` reaches
/// the same crash the binding leg fixed, but there is no binding to
/// annotate: the literal lives in a `return` and the receiver is a
/// call result.
///
/// Two forcing spellings were measured on the parent commit, and
/// only one works. Widening the fn's RETURN TYPE to `any` still
/// SIGSEGVs — that moves what the CALL SITE sees while the literal
/// itself keeps lowering nominal, so the marked set never gains it.
/// Wrapping the returned literal in `as any` answers everything
/// (bare detached call, `.call` with a receiver, direct
/// `mk().read()`, receiver writes through a bound result), because
/// the (c) leg's `lower_as_cast` promote makes the literal ACTUALLY
/// take the dynobj route while the cast also carries the type — the
/// marking and the repr move together. Marking alone is exactly the
/// silent-wrong the binding leg's doc warns about.
///
/// So the knife writes the cast the user could have written. Only a
/// `return` directly inside the named fn's own statement tree
/// counts; a return nested in a closure belongs to that closure and
/// the recursion does not enter Exprs. Fn names are not
/// scope-resolved, same trade the (f) leg takes: a same-named fn
/// elsewhere can force this one, which only ever costs the nominal
/// receiver's struct-offset reads.
pub(super) fn force_returned_literals(
    stmts: &mut [Stmt],
    exprs: &mut Vec<Expr>,
    spans: &mut Vec<crate::lexer::Span>,
    reads: &std::collections::HashSet<(String, String)>,
) {
    force_inner(stmts, exprs, spans, reads, None);
}

fn force_inner(
    stmts: &mut [Stmt],
    exprs: &mut Vec<Expr>,
    spans: &mut Vec<crate::lexer::Span>,
    reads: &std::collections::HashSet<(String, String)>,
    cur_fn: Option<&str>,
) {
    for s in stmts {
        match s {
            Stmt::FnDecl { name, body, .. } => {
                let inner = name.clone();
                force_inner(body, exprs, spans, reads, Some(&inner));
            }
            Stmt::Return(Some(eid)) => {
                if let Some(f) = cur_fn
                    && let Some(wrapped) = wrap_as_any(exprs, spans, reads, f, *eid)
                {
                    *eid = wrapped;
                }
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                force_inner(inner, exprs, spans, reads, cur_fn)
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                force_inner(
                    std::slice::from_mut(then_branch.as_mut()),
                    exprs,
                    spans,
                    reads,
                    cur_fn,
                );
                if let Some(eb) = else_branch {
                    force_inner(
                        std::slice::from_mut(eb.as_mut()),
                        exprs,
                        spans,
                        reads,
                        cur_fn,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                force_inner(
                    std::slice::from_mut(body.as_mut()),
                    exprs,
                    spans,
                    reads,
                    cur_fn,
                );
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    force_inner(
                        std::slice::from_mut(i.as_mut()),
                        exprs,
                        spans,
                        reads,
                        cur_fn,
                    );
                }
                force_inner(
                    std::slice::from_mut(body.as_mut()),
                    exprs,
                    spans,
                    reads,
                    cur_fn,
                );
            }
            _ => {}
        }
    }
}

/// Mint `<literal> as any` when this returned literal carries a
/// method the program read as a value off `fname()`. Returns the new
/// ExprId, or None when the return is not that literal.
fn wrap_as_any(
    exprs: &mut Vec<Expr>,
    spans: &mut Vec<crate::lexer::Span>,
    reads: &std::collections::HashSet<(String, String)>,
    fname: &str,
    eid: ExprId,
) -> Option<ExprId> {
    let Expr::ObjectLit { fields } = &exprs[eid.0 as usize] else {
        return None;
    };
    let detached = fields.iter().any(|(f, fe)| {
        reads.iter().any(|(rf, rm)| rf == fname && rm == f)
            && matches!(&exprs[fe.0 as usize], Expr::Closure { .. })
    });
    if !detached {
        return None;
    }
    // The cast inherits the literal's span (the `add_expr` sentinel
    // when the literal itself is synthetic).
    let span = spans
        .get(eid.0 as usize)
        .copied()
        .unwrap_or(crate::lexer::Span { start: 0, end: 0 });
    let id = ExprId(exprs.len() as u32);
    exprs.push(Expr::As {
        expr: eid,
        ty_ann: "any".to_string(),
    });
    spans.push(span);
    Some(id)
}
