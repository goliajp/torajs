//! §8.1.1.1 step 4 — the temporal dead zone a CLOSURE can reach, in
//! every ordinary statement list: a function value that reads a `let` /
//! `const` and may be called before the declaration has run.
//!
//! ```text
//! const x = (() => x)();              // ReferenceError
//! const o = { v: (() => o)() };       // ReferenceError
//! const f = () => y; f(); let y = 1;  // ReferenceError
//! ```
//!
//! tr answered `null`, an unknown-any tag, or `object` for these, or
//! refused the program outright — the closure read the binding's box
//! (or its data global) before anything was stored in it. The static
//! rule (`tdz_reads`) cannot help: a closure reads whenever it is
//! CALLED, which no statement order settles.
//!
//! The mechanism is the generator machine's (`desugar_generators_tdz`
//! documents it and `desugar_generators_tdz_flag` implements the site
//! walk and rewrite this pass reuses): the binding gets a runtime
//! initialised-flag, the "uninitialized" marker an engine keeps in the
//! binding slot, and each risky read becomes
//! `(__tdzcN_x ? undefined : <raise>, x)`. The flag is declared just
//! before the first statement holding a risky site and set right after
//! the declaration, both inside a transparent `Multi` so no index of
//! the list moves.
//!
//! What this pass adds is WHEN a flag is not needed, because the
//! shapes it would otherwise tax are the hot ones — a recursive arrow,
//! a pair of mutually recursive arrows, an object literal of methods
//! that name the object. A closure can only run if something calls it,
//! so a site is safe when nothing between the closure's creation and
//! the end of the declaration can run user code: every statement from
//! the one writing the closure through the declaration itself is
//! INERT — a function declaration, or a binding / assignment whose
//! value is built only from literals, arrows, and arrays / object
//! literals of those. Identifiers are not inert: a read of a global
//! object accessor runs a getter.
//!
//! Not entered, and a recorded gap rather than a guess: a `function`
//! declaration standing in the list itself. It is hoisted, so its
//! creation point is the top of the list and almost every later
//! binding it reads would carry a flag into what is usually a hot
//! function; settling it properly needs to know which declarations a
//! call before the binding can reach. A switch CaseBlock's own bindings
//! are settled by `switch_tdz_flag`.

use super::desugar_generators_tdz_flag::{
    Site, collect_sites_expr, collect_sites_stmt, rewrite_site,
};
use super::desugar_with::walk::arrow_nodes;
use super::{Ast, Expr, ExprId, StaticInit, Stmt};

/// Entry — every statement list of the program, outer lists first so a
/// closure body is still in the arena while the list around it scans.
pub fn flag_closure_tdz(ast: &mut Ast) {
    let mut counter = 0usize;
    let mut stmts = std::mem::take(&mut ast.stmts);
    flag_list(ast, &mut stmts, &mut counter);
    ast.stmts = stmts;
}

fn flag_list(ast: &mut Ast, stmts: &mut [Stmt], counter: &mut usize) {
    let mut paths: Vec<Vec<usize>> = Vec::new();
    flat_paths(stmts, &mut Vec::new(), &mut paths);
    let inert: Vec<bool> = paths
        .iter()
        .map(|p| stmt_inert(ast, at(stmts, p)))
        .collect();
    let mut decls: Vec<(usize, String)> = Vec::new();
    let mut heads: Vec<(usize, String)> = Vec::new();
    for j in 0..paths.len() {
        let Stmt::LetDecl {
            name,
            init,
            is_var: false,
            ..
        } = at(stmts, &paths[j])
        else {
            continue;
        };
        if name.starts_with("__") {
            continue;
        }
        let (name, init) = (name.clone(), *init);
        let mut sites: Vec<Site> = Vec::new();
        let mut first = j;
        for i in 0..j {
            let mut here = Vec::new();
            collect_sites_stmt(ast, at(stmts, &paths[i]), &name, false, false, &mut here);
            if !here.is_empty() && !inert[i..=j].iter().all(|b| *b) {
                first = first.min(i);
                sites.extend(here);
            }
        }
        let mut here = Vec::new();
        collect_sites_expr(ast, init, &name, false, false, &mut here);
        if !here.is_empty() && !inert[j] {
            sites.extend(here);
        }
        if sites.is_empty() {
            continue;
        }
        *counter += 1;
        let flag = format!("__tdzc{}_{name}", *counter);
        for site in sites {
            rewrite_site(ast, site, &name, &flag);
        }
        decls.push((j, flag.clone()));
        heads.push((first, flag));
    }
    for (j, flag) in decls {
        let target = ast.add_expr(Expr::Ident(flag));
        let value = ast.add_expr(Expr::Bool(true));
        let set = Stmt::Expr(ast.add_expr(Expr::Assign { target, value }));
        wrap(at_mut(stmts, &paths[j]), |s| vec![s, set]);
    }
    for (i, flag) in heads {
        let init = ast.add_expr(Expr::Bool(false));
        let decl = Stmt::LetDecl {
            mutable: true,
            name: flag,
            type_ann: Some("boolean".into()),
            init,
            is_var: false,
        };
        wrap(at_mut(stmts, &paths[i]), |s| vec![decl, s]);
    }
    for s in stmts.iter_mut() {
        descend(ast, s, counter);
    }
}

/// Every statement list nested in `s`: blocks, function and class
/// bodies, and the bodies of the closures its own expressions write.
fn descend(ast: &mut Ast, s: &mut Stmt, counter: &mut usize) {
    for a in arrow_nodes(ast, s) {
        flag_arrow(ast, a, counter);
    }
    match s {
        Stmt::Block(v) | Stmt::FnDecl { body: v, .. } => flag_list(ast, v, counter),
        Stmt::Multi(v) => {
            for c in v.iter_mut() {
                descend(ast, c, counter);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            flag_list(ast, body, counter);
            flag_list(ast, catch_body, counter);
            if let Some(f) = finally_body {
                flag_list(ast, f, counter);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            descend(ast, then_branch, counter);
            if let Some(e) = else_branch {
                descend(ast, e, counter);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => descend(ast, body, counter),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                descend(ast, i, counter);
            }
            descend(ast, body, counter);
        }
        Stmt::Switch {
            cases,
            default,
            default_at,
            ..
        } => {
            let heads =
                super::switch_tdz_flag::flag_case_block(ast, cases, default, *default_at, counter);
            for c in cases.iter_mut() {
                for b in c.body.iter_mut() {
                    descend(ast, b, counter);
                }
            }
            for b in default.iter_mut().flatten() {
                descend(ast, b, counter);
            }
            if !heads.is_empty() {
                wrap(s, |sw| heads.into_iter().chain([sw]).collect());
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => descend(ast, inner, counter),
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                flag_list(ast, &mut c.body, counter);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                flag_list(ast, &mut m.body, counter);
            }
            for i in static_init.iter_mut() {
                match i {
                    StaticInit::Block(b) => flag_list(ast, b, counter),
                    StaticInit::Field(f) => {
                        let mut e = Stmt::Expr(f.init);
                        descend(ast, &mut e, counter);
                    }
                }
            }
        }
        _ => {}
    }
}

/// A closure's body is a list of its own; it is lifted out of the arena
/// for the walk and put back.
fn flag_arrow(ast: &mut Ast, eid: ExprId, counter: &mut usize) {
    let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
        return;
    };
    let mut body = std::mem::take(body);
    flag_list(ast, &mut body, counter);
    if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[eid.0 as usize] {
        *slot = body;
    }
}

/// Index paths through `Multi` groups: the list is scanned as the one
/// scope it is without being restructured.
pub(super) fn flat_paths(stmts: &[Stmt], prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (i, s) in stmts.iter().enumerate() {
        prefix.push(i);
        match s {
            Stmt::Multi(inner) => flat_paths(inner, prefix, out),
            _ => out.push(prefix.clone()),
        }
        prefix.pop();
    }
}

pub(super) fn at<'a>(stmts: &'a [Stmt], path: &[usize]) -> &'a Stmt {
    let s = &stmts[path[0]];
    match (s, path.len()) {
        (_, 1) => s,
        (Stmt::Multi(inner), _) => at(inner, &path[1..]),
        _ => unreachable!("flat path descends only through Multi"),
    }
}

pub(super) fn at_mut<'a>(stmts: &'a mut [Stmt], path: &[usize]) -> &'a mut Stmt {
    if path.len() == 1 {
        return &mut stmts[path[0]];
    }
    match &mut stmts[path[0]] {
        Stmt::Multi(inner) => at_mut(inner, &path[1..]),
        _ => unreachable!("flat path descends only through Multi"),
    }
}

/// Replace `s` in place by a `Multi` holding what `f` makes of it. A
/// statement wrapped once is found again by the same path, so a second
/// wrap nests.
pub(super) fn wrap(s: &mut Stmt, f: impl FnOnce(Stmt) -> Vec<Stmt>) {
    let orig = std::mem::replace(s, Stmt::Multi(Vec::new()));
    *s = Stmt::Multi(f(orig));
}

/// Can running this statement call a closure? `true` only when it
/// provably cannot.
fn stmt_inert(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } => true,
        Stmt::LetDecl { init, .. } | Stmt::Expr(init) => expr_inert(ast, *init),
        _ => false,
    }
}

fn expr_inert(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Number(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::BigInt { .. }
        | Expr::Uninit
        | Expr::Elision
        | Expr::ArrowFn { .. } => true,
        Expr::Array(items) => items.iter().all(|e| expr_inert(ast, *e)),
        // A `__`-prefixed key is a sentinel for a spread, a computed
        // key, or an accessor — shapes that can run code.
        Expr::ObjectLit { fields } => fields
            .iter()
            .all(|(k, e)| !k.lossy().starts_with("__") && expr_inert(ast, *e)),
        Expr::Assign { target, value } => {
            matches!(ast.get_expr(*target), Expr::Ident(_)) && expr_inert(ast, *value)
        }
        _ => false,
    }
}
