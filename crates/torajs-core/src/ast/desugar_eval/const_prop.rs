//! Constant propagation into `eval` argument position — the extension
//! of the literal-concat fold to a NAMED source string:
//!
//!   var evalStr = '…' + '…';   →   eval(evalStr)  ≡  eval('…' + '…')
//!
//! which test262 writes constantly (S10.2.3_A* builds the source in a
//! `var`, the sm/eval exhaustive family builds it inside the test fn).
//! After this rewrite every downstream eval pass sees a plain literal
//! call and needs no knowledge of the binding.
//!
//! Safety is static, not approximate. A name propagates only when ALL
//! of the following hold:
//! - its declaration's initializer is a compile-time string
//!   (`const_string`: literal or a `+` tree of literals);
//! - the name is declared exactly ONCE in the whole program — a
//!   second declaration anywhere is a shadow that could rebind it;
//! - nothing ever assigns to it (`Assign` / `PostIncr` targets), and
//!   no parameter / loop variable / catch binding / fn / class / type
//!   name reuses it;
//! - the declaration sits in the INERT PREFIX of its statement list:
//!   every statement before it is a declaration (fn / type / import)
//!   or a let/var whose initializer cannot call user code. Any
//!   execution path entering that list therefore runs the declaration
//!   before it can reach a call — including a call to a closure
//!   defined earlier in the same prefix — so no use site can observe
//!   the pre-init value (`undefined` under var hoisting). A
//!   declaration past the first executable statement never
//!   propagates.

use std::collections::{HashMap, HashSet};

use super::super::{Ast, Expr, ExprId, Stmt};
use super::source::{callee_eval_form, const_string};

pub(super) fn propagate_eval_const_args(ast: &mut Ast) {
    let mut cands: HashMap<String, String> = HashMap::new();
    let mut decl_counts: HashMap<String, u32> = HashMap::new();
    scan_seq(&ast.stmts, ast, &mut cands, &mut decl_counts);
    if cands.is_empty() {
        return;
    }
    let mut tainted: HashSet<String> = HashSet::new();
    collect_taint(ast, &mut tainted, &mut decl_counts);
    cands.retain(|n, _| decl_counts.get(n) == Some(&1) && !tainted.contains(n));
    if cands.is_empty() {
        return;
    }
    for i in 0..ast.exprs.len() {
        let (callee, arg) = match &ast.exprs[i] {
            Expr::Call { callee, args } if args.len() == 1 => (*callee, args[0]),
            _ => continue,
        };
        if callee_eval_form(callee, ast).is_none() {
            continue;
        }
        let Some(Expr::Ident(n)) = ast.exprs.get(arg.0 as usize) else {
            continue;
        };
        let Some(val) = cands.get(n) else { continue };
        let lit = ast.add_expr(Expr::String(val.clone()));
        let Expr::Call { args, .. } = &mut ast.exprs[i] else {
            continue;
        };
        args[0] = lit;
    }
}

/// Walk every statement list. Candidates are collected only from a
/// list's inert prefix; declaration COUNTS are collected everywhere
/// (a same-named let in any scope is a shadow and kills the name).
fn scan_seq(
    stmts: &[Stmt],
    ast: &Ast,
    cands: &mut HashMap<String, String>,
    counts: &mut HashMap<String, u32>,
) {
    let mut prefix_inert = true;
    for s in stmts {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                *counts.entry(name.clone()).or_insert(0) += 1;
                if prefix_inert {
                    if let Some(v) = const_string(*init, ast) {
                        cands.insert(name.clone(), v);
                    } else if !init_inert(*init, ast) {
                        prefix_inert = false;
                    }
                }
            }
            Stmt::FnDecl { body, .. } => scan_seq(body, ast, cands, counts),
            Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => {}
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                ..
            } => {
                prefix_inert = false;
                if let Some(c) = ctor {
                    scan_seq(&c.body, ast, cands, counts);
                }
                for m in methods.iter().chain(static_methods) {
                    scan_seq(&m.body, ast, cands, counts);
                }
            }
            other => {
                prefix_inert = false;
                walk_nested(other, ast, counts);
            }
        }
    }
}

/// Statement lists nested under an executable statement (loop bodies,
/// branches, try arms…) never hold a propagatable declaration, but
/// still contribute shadow counts and may define fns of their own.
fn walk_nested(s: &Stmt, ast: &Ast, counts: &mut HashMap<String, u32>) {
    let mut bump = |lists: &[&[Stmt]]| {
        for l in lists {
            let mut inner_cands = HashMap::new();
            scan_seq(l, ast, &mut inner_cands, counts);
        }
    };
    match s {
        Stmt::Block(b) | Stmt::Multi(b) => bump(&[b]),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            bump(&[std::slice::from_ref(then_branch)]);
            if let Some(e) = else_branch {
                bump(&[std::slice::from_ref(e)]);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. } => bump(&[std::slice::from_ref(body)]),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                bump(&[std::slice::from_ref(i)]);
            }
            bump(&[std::slice::from_ref(body)]);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            bump(&[body, catch_body]);
            if let Some(f) = finally_body {
                bump(&[f]);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                bump(&[&c.body]);
            }
            if let Some(d) = default {
                bump(&[d]);
            }
        }
        _ => {}
    }
}

/// An initializer that cannot run user code: a literal, a bare name
/// (`var ev = eval`), or a fold-able string tree.
fn init_inert(eid: ExprId, ast: &Ast) -> bool {
    if const_string(eid, ast).is_some() {
        return true;
    }
    matches!(
        ast.exprs.get(eid.0 as usize),
        Some(
            Expr::Ident(_)
                | Expr::Number(_)
                | Expr::Bool(_)
                | Expr::Null
                | Expr::Uninit
                | Expr::ArrowFn { .. }
        )
    )
}

/// Names that are ever written, rebound, or introduced by a non-let
/// binding form anywhere in the program. Arrow-fn bodies live in the
/// expression arena — scan_seq never enters them, so their let
/// declarations register their shadow COUNTS here instead (an arrow
/// body's own `let s` must kill an outer candidate `s` its `eval(s)`
/// would otherwise resolve to).
fn collect_taint(ast: &Ast, out: &mut HashSet<String>, counts: &mut HashMap<String, u32>) {
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                if let Some(Expr::Ident(n)) = ast.exprs.get(target.0 as usize) {
                    out.insert(n.clone());
                }
            }
            Expr::ArrowFn { params, body, .. } => {
                for p in params {
                    out.insert(p.name.clone());
                }
                count_letdecls(body, counts);
                collect_stmt_taint(body, out);
            }
            _ => {}
        }
    }
    collect_stmt_taint(&ast.stmts, out);
}

/// Shadow counter over a statement tree (arrow bodies only — the
/// scan_seq walk already counts every list it enters).
fn count_letdecls(stmts: &[Stmt], counts: &mut HashMap<String, u32>) {
    for s in stmts {
        if let Stmt::LetDecl { name, .. } = s {
            *counts.entry(name.clone()).or_insert(0) += 1;
        }
        for_each_child_list(s, &mut |l| count_letdecls(l, counts));
    }
}

/// Visit every direct child statement list of `s`.
fn for_each_child_list(s: &Stmt, f: &mut dyn FnMut(&[Stmt])) {
    match s {
        Stmt::Block(b) | Stmt::Multi(b) => f(b),
        Stmt::FnDecl { body, .. } => f(body),
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
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => f(std::slice::from_ref(body)),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                f(std::slice::from_ref(i));
            }
            f(std::slice::from_ref(body));
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            f(body);
            f(catch_body);
            if let Some(fb) = finally_body {
                f(fb);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                f(&c.body);
            }
            if let Some(d) = default {
                f(d);
            }
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            ..
        } => {
            if let Some(c) = ctor {
                f(&c.body);
            }
            for m in methods.iter().chain(static_methods) {
                f(&m.body);
            }
        }
        Stmt::ExportDecl { inner: Some(i), .. } => f(std::slice::from_ref(i)),
        _ => {}
    }
}

fn collect_stmt_taint(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                out.insert(name.clone());
                for p in params {
                    out.insert(p.name.clone());
                }
                collect_stmt_taint(body, out);
            }
            Stmt::ClassDecl {
                name,
                ctor,
                methods,
                static_methods,
                ..
            } => {
                out.insert(name.clone());
                if let Some(c) = ctor {
                    for p in &c.params {
                        out.insert(p.name.clone());
                    }
                    collect_stmt_taint(&c.body, out);
                }
                for m in methods.iter().chain(static_methods) {
                    for p in &m.params {
                        out.insert(p.name.clone());
                    }
                    collect_stmt_taint(&m.body, out);
                }
            }
            Stmt::TypeDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::ForOf {
                var_name,
                i_ident,
                body,
                ..
            } => {
                out.insert(var_name.clone());
                out.insert(i_ident.clone());
                collect_stmt_taint(std::slice::from_ref(body), out);
            }
            Stmt::ForOfSplitIter { var_name, body, .. } => {
                out.insert(var_name.clone());
                collect_stmt_taint(std::slice::from_ref(body), out);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                if let Some(p) = catch_param {
                    out.insert(p.clone());
                }
                collect_stmt_taint(body, out);
                collect_stmt_taint(catch_body, out);
                if let Some(f) = finally_body {
                    collect_stmt_taint(f, out);
                }
            }
            Stmt::YieldInto { var, .. } => {
                out.insert(var.clone());
            }
            Stmt::ImportDecl {
                default,
                namespace,
                named,
                ..
            } => {
                if let Some(d) = default {
                    out.insert(d.clone());
                }
                if let Some(n) = namespace {
                    out.insert(n.clone());
                }
                for (a, b) in named {
                    out.insert(b.clone().unwrap_or_else(|| a.clone()));
                }
            }
            Stmt::Block(b) | Stmt::Multi(b) => collect_stmt_taint(b, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_stmt_taint(std::slice::from_ref(then_branch), out);
                if let Some(e) = else_branch {
                    collect_stmt_taint(std::slice::from_ref(e), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                collect_stmt_taint(std::slice::from_ref(body), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_stmt_taint(std::slice::from_ref(i), out);
                }
                collect_stmt_taint(std::slice::from_ref(body), out);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_stmt_taint(&c.body, out);
                }
                if let Some(d) = default {
                    collect_stmt_taint(d, out);
                }
            }
            Stmt::ExportDecl { inner, .. } => {
                if let Some(i) = inner {
                    collect_stmt_taint(std::slice::from_ref(i), out);
                }
            }
            _ => {}
        }
    }
}
