//! The shared nested-statement-list spine — every compound
//! statement's bodies (fn decls, blocks, try/if/loops/switch) handed
//! to a callback in syntactic order. A by-name collector built on it
//! re-finds a binding wherever the program put it (an if-branch, a
//! loop body, the Try that `desugar_async` wraps an async body in);
//! one that hand-rolls its recursion skips a compound form sooner or
//! later and silently mis-judges the binding inside it (rotation
//! 345's any-let lesson, hit again in rotation 346 when the
//! capturing-lane `const` for a nested FnDecl-this landed inside the
//! async Try and never reached the promote candidate set).

use super::Stmt;

/// 393-03 — the mutable twin of [`for_each_nested_list`], fully
/// recursive: every nested `Vec<Stmt>` container of `s` (same
/// variant coverage as the read-only spine), outermost first, then
/// each element's own containers. The callback may replace elements
/// in place (the generator lift swaps a claimed FnDecl for an empty
/// Block); replacements are themselves descended into afterwards.
/// Single-Stmt positions (a bare `if (c) function f() {}` branch)
/// are traversed through but are not themselves a list — a decl
/// sitting directly there is not offered to the callback, matching
/// the read-only spine's shape.
pub(crate) fn for_each_nested_vec_mut(s: &mut Stmt, f: &mut dyn FnMut(&mut Vec<Stmt>)) {
    fn visit_vec(v: &mut Vec<Stmt>, f: &mut dyn FnMut(&mut Vec<Stmt>)) {
        f(v);
        for s in v.iter_mut() {
            for_each_nested_vec_mut(s, f);
        }
    }
    match s {
        Stmt::FnDecl { body, .. } | Stmt::Block(body) | Stmt::Multi(body) => visit_vec(body, f),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            visit_vec(body, f);
            visit_vec(catch_body, f);
            if let Some(fin) = finally_body {
                visit_vec(fin, f);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for_each_nested_vec_mut(then_branch, f);
            if let Some(e) = else_branch {
                for_each_nested_vec_mut(e, f);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            for_each_nested_vec_mut(body, f)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                for_each_nested_vec_mut(i, f);
            }
            for_each_nested_vec_mut(body, f);
        }
        Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
            for_each_nested_vec_mut(body, f)
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                visit_vec(&mut c.body, f);
            }
            if let Some(d) = default {
                visit_vec(d, f);
            }
        }
        _ => {}
    }
}

/// Every nested statement list of `s`, in syntactic order.
pub(crate) fn for_each_nested_list<'a>(s: &'a Stmt, f: &mut dyn FnMut(&'a [Stmt])) {
    match s {
        Stmt::FnDecl { body, .. } => f(body),
        Stmt::Block(inner) | Stmt::Multi(inner) => f(inner),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            f(body);
            f(catch_body);
            if let Some(fin) = finally_body {
                f(fin);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            f(std::slice::from_ref(then_branch));
            if let Some(e) = else_branch {
                f(std::slice::from_ref(e));
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            f(std::slice::from_ref(body))
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                f(std::slice::from_ref(i));
            }
            f(std::slice::from_ref(body));
        }
        Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
            f(std::slice::from_ref(body))
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                f(&c.body);
            }
            if let Some(d) = default {
                f(d);
            }
        }
        _ => {}
    }
}
