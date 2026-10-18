//! Declared functions whose signature names the class or a container:
//! each must be called only directly, with arguments of exactly its
//! parameter readings, and must answer one of its return reading on
//! every path.

use std::collections::HashMap;

use crate::ast::{Expr, ExprId, Stmt};

use super::super::program::{Cat, Holder, Program};
use crate::ssa_lower_pass_0_5::live_field_census::walk::own_returns;

pub(super) struct Watched {
    /// Per parameter, its reading when it names the class.
    pub params: Vec<Option<Cat>>,
    pub ret: Option<Cat>,
}

/// The watched functions, or why one cannot be.
pub(super) fn watched<'a>(
    prog: &Program<'a>,
    h: &Holder<'a>,
) -> Result<HashMap<&'a str, Watched>, String> {
    let mut out = HashMap::new();
    let c = h.class;
    // in name order, so the reason a class is ruled out is the same
    // on every run
    let mut fns: Vec<_> = prog.fns.iter().collect();
    fns.sort_unstable_by_key(|(n, _)| **n);
    for (name, f) in fns {
        if name.strip_prefix("__new_") == Some(c)
            || name
                .strip_prefix("__cm_")
                .and_then(|n| n.strip_suffix("__ctor"))
                == Some(c)
        {
            continue;
        }
        let params: Vec<Option<Cat>> = f
            .params
            .iter()
            .map(|p| p.type_ann.as_deref().and_then(|a| h.ann_cat(a)))
            .collect();
        let ret = f.ret.and_then(|a| h.ann_cat(a));
        let own_ret = own_ret_cat(prog, h, f.body);
        if params.iter().all(Option::is_none) && ret.is_none() && own_ret.is_none() {
            continue;
        }
        let shape_ok = |c: &Option<Cat>| matches!(c, None | Some(Cat::Arr | Cat::Boxed(_)));
        if f.decls != 1 || f.generic || prog.bound.contains(name) {
            return Err(format!(
                "`{name}` names it but is not one plain declaration"
            ));
        }
        if !params.iter().all(shape_ok)
            || f.params
                .iter()
                .zip(&params)
                .any(|(p, c)| c.is_some() && (p.is_rest || p.default.is_some()))
        {
            return Err(format!("a parameter of `{name}` holds it in another shape"));
        }
        if (ret.is_some() || own_ret.is_some())
            && (!shape_ok(&ret) || ret.is_none() || !answers_on_every_path(prog, h, f.body, ret))
        {
            return Err(format!(
                "`{name}` does not answer it as declared on every path"
            ));
        }
        out.insert(*name, Watched { params, ret });
    }
    Ok(out)
}

/// What some own `return` of the body answers, when it holds anything.
fn own_ret_cat(prog: &Program<'_>, h: &Holder<'_>, body: &[Stmt]) -> Option<Cat> {
    let mut out = None;
    own_returns(body, &mut |v| {
        if out.is_none()
            && let Some(e) = v
        {
            out = h.cat_of(prog, e);
        }
    });
    out
}

/// Every own `return` answers a value of reading `ret`, and the body
/// cannot run off its end.
fn answers_on_every_path(
    prog: &Program<'_>,
    h: &Holder<'_>,
    body: &[Stmt],
    ret: Option<Cat>,
) -> bool {
    let mut ok = true;
    own_returns(body, &mut |v| {
        ok &= v.is_some_and(|e| h.cat_of(prog, e) == ret);
    });
    let last = body
        .iter()
        .rev()
        .find(|s| !matches!(s, Stmt::Expr(e) if matches!(prog.ast.get_expr(*e), Expr::String(_))));
    ok && matches!(last, Some(Stmt::Return(_) | Stmt::Throw(_)))
}

/// A direct call of a watched function: every parameter that names the
/// class gets an argument of exactly that reading.
pub(super) fn call_site_ok(
    prog: &Program<'_>,
    h: &Holder<'_>,
    w: &Watched,
    args: &[ExprId],
) -> bool {
    if args
        .iter()
        .any(|a| matches!(prog.ast.get_expr(*a), Expr::Spread { .. }))
    {
        return false;
    }
    w.params.iter().enumerate().all(|(i, want)| match want {
        None => true,
        Some(want) => args
            .get(i)
            .is_some_and(|a| h.cat_of(prog, *a) == Some(*want)),
    })
}
