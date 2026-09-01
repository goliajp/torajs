//! The rewrite half of [`crate::ast_closure_param_tag`]. That module
//! decides WHICH fn-typed params and returns carry closure repr; this
//! one applies the decision to their annotations in place, and carries
//! the mutable statement walk the rewrites need.

use crate::ast::Stmt;
use std::collections::HashSet;

/// `__fn(` → `__cls(` on one annotation, reaching through a
/// `__nullable(` wrapper when there is one. `T | null` for a
/// pointer-shaped T IS that T once lowered — only `number` and
/// `boolean` box (`ssa_lower_parse_type::nullable_inner_boxes`),
/// everything else keeps its own repr with null as the in-band 0 — so
/// the payload is what carries the repr tag, and the wrapper has to
/// survive the rewrite rather than be replaced by it.
fn retag_to_cls(ann: &mut String) {
    if let Some(rest) = ann.strip_prefix("__fn(") {
        *ann = format!("__cls({rest}");
    } else if let Some(rest) = ann.strip_prefix("__nullable(__fn(") {
        *ann = format!("__nullable(__cls({rest}");
    }
}

/// Apply the retag on every ret-marked FnDecl's return type.
pub(crate) fn retag_fn_return_types(stmts: &mut [Stmt], ret_marked: &HashSet<String>) {
    let mut stack: Vec<&mut Stmt> = stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name, return_type, ..
        } = s
            && ret_marked.contains(name)
            && let Some(ann) = return_type
        {
            retag_to_cls(ann);
        }
        push_child_stmts_mut(s, &mut stack);
    }
}

/// Apply the retag on every marked (fn, param idx).
pub(crate) fn retag_fn_decls(stmts: &mut [Stmt], marked: &HashSet<(String, usize)>) {
    let mut stack: Vec<&mut Stmt> = stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl { name, params, .. } = s {
            for (i, p) in params.iter_mut().enumerate() {
                if marked.contains(&(name.clone(), i))
                    && let Some(ann) = &mut p.type_ann
                {
                    retag_to_cls(ann);
                }
            }
        }
        push_child_stmts_mut(s, &mut stack);
    }
}

/// Mutable twin of [`crate::ast_closure_param_tag::push_child_stmts`].
fn push_child_stmts_mut<'a>(s: &'a mut Stmt, stack: &mut Vec<&'a mut Stmt>) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stack.push(then_branch);
            if let Some(e) = else_branch {
                stack.push(e);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stack.push(body),
        Stmt::Labeled { body, .. } => stack.push(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                stack.push(i);
            }
            stack.push(body);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => stack.push(body),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                stack.extend(c.body.iter_mut());
            }
            if let Some(d) = default {
                stack.extend(d.iter_mut());
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            stack.extend(body.iter_mut());
            stack.extend(catch_body.iter_mut());
            if let Some(f) = finally_body {
                stack.extend(f.iter_mut());
            }
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => stack.extend(inner.iter_mut()),
        Stmt::FnDecl { body, .. } => stack.extend(body.iter_mut()),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner {
                stack.push(i);
            }
        }
        _ => {}
    }
}
