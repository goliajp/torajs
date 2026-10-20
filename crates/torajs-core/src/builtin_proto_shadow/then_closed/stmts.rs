//! The statement walk of the `then` closure proof: the names a
//! program binds, declares or exports.

use super::{Opening, open};
use crate::ast::Stmt;

/// The names a statement binds or exports: none may be `then`, and no
/// class member the class lowering names (`__cm_<C>__then`,
/// `__sm_<C>__then_get`, `__sf_<C>__then`) either.
pub(super) fn stmt_opening(s: &Stmt) -> Option<Opening> {
    let named = |n: &str| {
        let base = (n.strip_suffix("_get"))
            .or_else(|| n.strip_suffix("_set"))
            .unwrap_or(n);
        (n == "then" || base.ends_with("__then")).then(|| Opening {
            why: "binding",
            at: n.to_string(),
        })
    };
    let all = |ss: &[Stmt]| ss.iter().find_map(stmt_opening);
    let one = |s: &Stmt| stmt_opening(s);
    match s {
        Stmt::FnDecl { name, body, .. } => named(name).or_else(|| all(body)),
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => named(name),
        Stmt::YieldInto { var, .. } => named(var),
        Stmt::ClassDecl { name, .. } => open("class", name.clone()),
        Stmt::ExportDecl {
            inner,
            named: names,
            ..
        } => (names.iter())
            .find_map(|(a, b)| named(a).or_else(|| b.as_deref().and_then(named)))
            .or_else(|| inner.as_deref().and_then(one)),
        Stmt::ImportDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::Expr(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Throw(_)
        | Stmt::Return(_)
        | Stmt::Yield(_) => None,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => one(then_branch).or_else(|| else_branch.as_deref().and_then(one)),
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => one(body),
        Stmt::ForOf { var_name, body, .. } => named(var_name).or_else(|| one(body)),
        Stmt::For { init, body, .. } => init.as_deref().and_then(one).or_else(|| one(body)),
        Stmt::Switch { cases, default, .. } => (cases.iter())
            .find_map(|c| all(&c.body))
            .or_else(|| default.as_deref().and_then(all)),
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => all(body)
            .or_else(|| catch_param.as_deref().and_then(named))
            .or_else(|| all(catch_body))
            .or_else(|| finally_body.as_deref().and_then(all)),
        Stmt::Block(ss) | Stmt::Multi(ss) => all(ss),
    }
}
