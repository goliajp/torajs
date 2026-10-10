//! A return through the closure lane hands back a fn object, never a
//! bare function: a const binding of a top-level function is returned
//! as that function, whose fn object the return answers, and a bare
//! fn-typed param returned there takes the closure repr, so each call
//! converts its argument to a fn object. The forwarders those objects
//! run were minted before the check, wherever the lane could be taken
//! (`ast_collect_fn_closure_target::raw_ret`).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ast_collect_fn_closure_target::fn_returns;
use crate::ast_collect_fn_closure_target::raw_ret::{RawRet, const_lets, raw_ret};

/// `lane`: the declarations `any_ret` retags; a declaration already
/// annotated with the closure repr takes the lane too.
pub(super) fn hand_back_cells(ast: &mut Ast, lane: &HashSet<String>) {
    let decls = crate::ast::toplevel_stmts_flat(ast);
    let fns: HashSet<&str> = decls
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let is_fn = |n: &str| fns.contains(n);
    let mut marks: HashSet<(String, usize)> = HashSet::new();
    let mut aliases: Vec<(ExprId, String)> = Vec::new();
    for s in &decls {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = s
        else {
            continue;
        };
        let cls = return_type
            .as_deref()
            .is_some_and(|a| a.starts_with("__cls(") || a.starts_with("__nullable(__cls("));
        if !cls && !lane.contains(name) {
            continue;
        }
        let lets = const_lets(body);
        for r in fn_returns(body) {
            match raw_ret(ast, params, &lets, r, &is_fn) {
                Some(RawRet::Param(i)) => {
                    marks.insert((name.clone(), i));
                }
                Some(RawRet::Named(o)) => {
                    let mut e = r;
                    while let Expr::As { expr, .. } = ast.get_expr(e) {
                        e = *expr;
                    }
                    if !matches!(ast.get_expr(e), Expr::Ident(k) if *k == o) {
                        aliases.push((e, o));
                    }
                }
                None => {}
            }
        }
    }
    let marks = with_callers_of_same_shape(ast, marks);
    for (e, o) in aliases {
        ast.exprs[e.0 as usize] = Expr::Ident(o);
    }
    if !marks.is_empty() {
        crate::ast_closure_param_tag_retag::retag_fn_decls(&mut ast.stmts, &marks);
    }
}

/// A param's repr is part of how every caller reaches it: the other
/// rows of its vtable slot and the slot's dispatch stubs take the same
/// index, and the declaration's forwarder shells the param of the
/// same name.
fn with_callers_of_same_shape(
    ast: &Ast,
    mut marks: HashSet<(String, usize)>,
) -> HashSet<(String, usize)> {
    if marks.is_empty() {
        return marks;
    }
    let params: HashMap<&str, Vec<&str>> = crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, params, .. } => Some((
                name.as_str(),
                params.iter().map(|p| p.name.as_str()).collect(),
            )),
            _ => None,
        })
        .collect();
    let names: Vec<String> = params.keys().map(|k| k.to_string()).collect();
    for (m, by_root) in crate::ast::vtable_slot::slot_groups_by_name(ast) {
        let mut all: HashSet<usize> = HashSet::new();
        for rows in by_root.values() {
            let idx: HashSet<usize> = marks
                .iter()
                .filter(|(f, _)| rows.contains(f))
                .map(|(_, i)| *i)
                .collect();
            for r in rows {
                marks.extend(idx.iter().map(|&i| (r.clone(), i)));
            }
            all.extend(idx);
        }
        for stub in crate::ast::vtable_slot::dispatch_stub_names(names.iter(), &m) {
            marks.extend(all.iter().map(|&i| (stub.clone(), i)));
        }
    }
    let mut shells: Vec<(String, usize)> = Vec::new();
    for (f, i) in &marks {
        let pname = params.get(f.as_str()).and_then(|ps| ps.get(*i)).copied();
        for sh in crate::ast::shell_names(f) {
            if let Some(j) = params
                .get(sh.as_str())
                .and_then(|ps| ps.iter().position(|p| Some(*p) == pname))
            {
                shells.push((sh, j));
            }
        }
    }
    marks.extend(shells);
    marks
}
