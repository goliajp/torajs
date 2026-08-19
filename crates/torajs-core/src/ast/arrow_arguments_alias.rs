//! Arrow-body `arguments` inheritance (ES §9.4.4 — an arrow has no
//! `arguments` binding of its own; a read resolves to the nearest
//! enclosing non-arrow function's). L3b ③, rotation 411 probe:
//! `function outer() { const f = () => arguments.length; return f() }`
//! answered 0 (bun: the outer argc) because `lift_arrow_fns` runs
//! BEFORE `desugar_arguments_object` — the arrow body, `arguments`
//! read included, moves into a fresh `__closure_N` FnDecl, and the
//! arguments pass then serves that fn's OWN (empty) argc.
//!
//! The fix is the textbook one (Babel's arrow transform does the
//! same): before the lift, alias the outer fn's `arguments` into an
//! ordinary local and point every arrow-interior read at it —
//!
//! ```text
//! function outer() {          function outer() {
//!   const f = () =>      →      let __arrowargs: any = arguments;
//!     arguments.length;         const f = () => __arrowargs.length;
//! }                           }
//! ```
//!
//! Everything downstream then works unchanged: the outer body's
//! `arguments` read makes `desugar_arguments_object` materialize the
//! object, and `lift_arrow_fns`' free-var walk captures the local
//! into the closure env. The alias name is deliberately ONE fixed
//! spelling: a nested non-arrow fn that needs its own alias declares
//! its own `__arrowargs`, and ordinary `let` shadowing IS the
//! nearest-enclosing-function rule.
//!
//! Scope walk: an `ArrowFn` body stays in the CURRENT fn's scope
//! (that is the point); a nested `Stmt::FnDecl` opens its own scope
//! and recurses independently. A miss in either direction is loud
//! downstream (the bare `arguments` ident stays unresolved), never
//! silently wrong.

use super::expr::{Expr, Param};
use super::stmt::Stmt;
use super::{Ast, ExprId};

/// Where the walk stands relative to arrow boundaries.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Inside the fn body proper — reads here are the fn's own.
    Outer,
    /// Crossed >=1 arrow boundary — reads belong to the enclosing fn.
    Arrow,
    /// No `arguments` object is reachable from here: either a formal
    /// parameter named `arguments` (legal in sloppy code — §14.2.16,
    /// the t262 lexical-bindings-overriden case) shadows the object
    /// for the whole lexical subtree, or the walk is at the top
    /// level, which is not a function scope at all. Nothing here may
    /// be rewritten, however many more arrows the walk crosses; an
    /// own-scope fn-expr still opens a fresh scope as usual.
    Shadowed,
}

/// Does a param list bind the name `arguments`?
fn params_shadow(params: &[Param]) -> bool {
    params.iter().any(|p| p.name == "arguments")
}

pub fn alias_arrow_arguments(ast: &mut Ast) {
    let n = ast.stmts.len();
    for i in 0..n {
        if matches!(ast.stmts[i], Stmt::FnDecl { .. }) {
            let Stmt::FnDecl { params, body, .. } = &mut ast.stmts[i] else {
                unreachable!()
            };
            let shadowed = params_shadow(params);
            let mut b = std::mem::take(body);
            process_scope(ast, &mut b, shadowed);
            let Stmt::FnDecl { body, .. } = &mut ast.stmts[i] else {
                unreachable!()
            };
            *body = b;
        } else {
            // The top level is not a function scope — there is no
            // `arguments` to alias here, so the walk starts in
            // Shadowed (nothing may be rewritten at this depth). The
            // walk exists to reach fn-exprs stored by top-level
            // statements (`Promise.resolve = function () {...}`,
            // `const f = function () {...}`): their own-scope branch
            // in `scan_expr` opens a real fn scope and aliases
            // arrow-interior `arguments` reads exactly as if the
            // fn-expr sat inside a FnDecl body.
            let mut s = std::mem::replace(&mut ast.stmts[i], Stmt::Multi(Vec::new()));
            let mut hits: Vec<ExprId> = Vec::new();
            scan_stmt(ast, &mut s, Mode::Shadowed, &mut hits);
            ast.stmts[i] = s;
        }
    }
}

/// One non-arrow function scope: collect every `arguments` ident
/// sitting inside an arrow subtree of `body`, rewrite them to the
/// alias spelling, and inject the alias binding at the body head
/// when any were found.
fn process_scope(ast: &mut Ast, body: &mut Vec<Stmt>, shadowed: bool) {
    let start = if shadowed {
        Mode::Shadowed
    } else {
        Mode::Outer
    };
    let mut hits: Vec<ExprId> = Vec::new();
    for s in body.iter_mut() {
        scan_stmt(ast, s, start, &mut hits);
    }
    if hits.is_empty() {
        return;
    }
    for eid in hits {
        ast.exprs[eid.0 as usize] = Expr::Ident("__arrowargs".to_string());
    }
    let init = ast.add_expr(Expr::Ident("arguments".to_string()));
    body.insert(
        0,
        Stmt::LetDecl {
            mutable: false,
            name: "__arrowargs".to_string(),
            type_ann: Some("any".to_string()),
            init,
            is_var: false,
        },
    );
}

/// Statement walk within one fn scope (`mode` — see [`Mode`]). A
/// nested `FnDecl` is its own scope — recurse through
/// [`process_scope`] and do not extend the current one.
fn scan_stmt(ast: &mut Ast, s: &mut Stmt, mode: Mode, hits: &mut Vec<ExprId>) {
    match s {
        Stmt::FnDecl { params, body, .. } => {
            let shadowed = params_shadow(params);
            let mut b = std::mem::take(body);
            process_scope(ast, &mut b, shadowed);
            *body = b;
        }
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => scan_expr(ast, *e, mode, hits),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                scan_expr(ast, *e, mode, hits);
            }
        }
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            scan_expr(ast, *init, mode, hits);
        }
        Stmt::YieldInto { value, .. } => scan_expr(ast, *value, mode, hits),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            scan_expr(ast, *cond, mode, hits);
            scan_stmt(ast, then_branch, mode, hits);
            if let Some(e) = else_branch {
                scan_stmt(ast, e, mode, hits);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            scan_expr(ast, *cond, mode, hits);
            scan_stmt(ast, body, mode, hits);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            scan_expr(ast, *scrutinee, mode, hits);
            for c in cases.iter_mut() {
                scan_expr(ast, c.value, mode, hits);
                for cs in c.body.iter_mut() {
                    scan_stmt(ast, cs, mode, hits);
                }
            }
            if let Some(d) = default {
                for ds in d.iter_mut() {
                    scan_stmt(ast, ds, mode, hits);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(s0) = init {
                scan_stmt(ast, s0, mode, hits);
            }
            if let Some(c) = cond {
                scan_expr(ast, *c, mode, hits);
            }
            if let Some(st) = step {
                scan_expr(ast, *st, mode, hits);
            }
            scan_stmt(ast, body, mode, hits);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            scan_expr(ast, *parent, mode, hits);
            scan_expr(ast, *sep, mode, hits);
            scan_stmt(ast, body, mode, hits);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            scan_expr(ast, *elem_expr, mode, hits);
            scan_stmt(ast, body, mode, hits);
        }
        Stmt::Labeled { body, .. } => scan_stmt(ast, body, mode, hits),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for bs in body.iter_mut() {
                scan_stmt(ast, bs, mode, hits);
            }
            for cs in catch_body.iter_mut() {
                scan_stmt(ast, cs, mode, hits);
            }
            if let Some(fb) = finally_body {
                for fs in fb.iter_mut() {
                    scan_stmt(ast, fs, mode, hits);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for bs in stmts.iter_mut() {
                scan_stmt(ast, bs, mode, hits);
            }
        }
        _ => {}
    }
}

/// Expression walk. Recursion collects child ids first (the arena is
/// borrowed immutably per node), then descends; a true arrow's body
/// is taken out of the node, walked in [`Mode::Arrow`] (or
/// [`Mode::Shadowed`] under an `arguments`-named param), and put
/// back.
fn scan_expr(ast: &mut Ast, eid: ExprId, mode: Mode, hits: &mut Vec<ExprId>) {
    // `Expr::ArrowFn` is a lossy encoding (see `parser/fn_expr.rs`
    // knife-1 note): a FUNCTION expression parses to the same node,
    // marked in `fn_expr_exprs` / `gen_fn_exprs` — and a fn-expr has
    // its OWN `arguments` binding, so its body is a fresh scope, not
    // an extension of this one (the un-gated first version rewrote
    // `const h = function () { return arguments[0] }` at the outer
    // fn's — six gate reds). Only the true arrow stays flagged.
    if matches!(ast.get_expr(eid), Expr::ArrowFn { .. }) {
        let own_scope = ast.fn_expr_exprs.contains(&eid) || ast.gen_fn_exprs.contains_key(&eid);
        let Expr::ArrowFn { params, body, .. } = &mut ast.exprs[eid.0 as usize] else {
            unreachable!()
        };
        let shadowed = params_shadow(params);
        let mut b = std::mem::take(body);
        if own_scope {
            process_scope(ast, &mut b, shadowed);
        } else {
            // A shadow anywhere up the arrow chain covers the whole
            // subtree; otherwise this is (or stays) the flagged walk.
            let inner = if shadowed || mode == Mode::Shadowed {
                Mode::Shadowed
            } else {
                Mode::Arrow
            };
            for s in b.iter_mut() {
                scan_stmt(ast, s, inner, hits);
            }
        }
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[eid.0 as usize] else {
            unreachable!()
        };
        *body = b;
        return;
    }
    if mode == Mode::Arrow && matches!(ast.get_expr(eid), Expr::Ident(n) if n == "arguments") {
        hits.push(eid);
        return;
    }
    let children: Vec<ExprId> = match ast.get_expr(eid) {
        Expr::BinOp { left, right, .. } => vec![*left, *right],
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::Delete { expr, .. }
        | Expr::As { expr, .. } => vec![*expr],
        Expr::PostIncr { target, .. } => vec![*target],
        Expr::Member { obj, .. } => vec![*obj],
        Expr::Index { obj, index } => vec![*obj, *index],
        Expr::Call { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        Expr::Assign { target, value } => vec![*target, *value],
        Expr::Array(items) => items.clone(),
        Expr::ObjectLit { fields } => fields.iter().map(|(_, e)| *e).collect(),
        Expr::New { args, .. } | Expr::Super { args } => args.clone(),
        Expr::NewDynamic { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => vec![*cond, *then_branch, *else_branch],
        Expr::Sequence { left, right } => vec![*left, *right],
        Expr::Nullish { lhs, rhs } => vec![*lhs, *rhs],
        Expr::InstanceOf { expr, rhs } => vec![*expr, *rhs],
        Expr::OptChain { obj, .. } => vec![*obj],
        Expr::OptIndex { obj, index } => vec![*obj, *index],
        Expr::OptCall { callee, args } => {
            let mut v = vec![*callee];
            v.extend(args.iter().copied());
            v
        }
        _ => Vec::new(),
    };
    for c in children {
        scan_expr(ast, c, mode, hits);
    }
}
