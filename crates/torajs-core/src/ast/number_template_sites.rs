//! Template-site numbering — gives every
//! `__torajs_template_object(-1, …)` call the program-unique site id
//! §13.2.8.4's per-site template-object cache keys on.
//!
//! The parser leaves a `-1` placeholder: a per-Parser counter would
//! collide across modules, and the runtime treats a negative site as
//! "uncached, build fresh" (the reduced REPL/LSP pipelines never run
//! this pass, and a fresh object every evaluation is the safe
//! degradation there). This pass runs once after modules splice and
//! renumbers in ARENA ORDER — parse order, deterministic across
//! builds (no hash iteration anywhere).
//!
//! Checker-side monomorph body clones share the numbered call's
//! sub-expressions, so a cloned body keeps its source site's id —
//! which is exactly §13.2.8.4: the cache keys on the source parse
//! node, not on the specialization that evaluates it.

use super::ast_def::Ast;
use crate::ast::Expr;

pub fn number_template_sites(ast: &mut Ast) {
    // Collect first (arena order), then rewrite — the arena can't be
    // walked and mutated at once.
    let mut site_args: Vec<usize> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let Expr::Ident(n) = &ast.exprs[callee.0 as usize] else {
            continue;
        };
        if n != crate::parser::TEMPLATE_OBJECT_CALLEE {
            continue;
        }
        let Some(first) = args.first() else { continue };
        if matches!(&ast.exprs[first.0 as usize], Expr::Number(v) if *v < 0.0) {
            site_args.push(first.0 as usize);
        }
    }
    for (site, slot) in site_args.into_iter().enumerate() {
        ast.exprs[slot] = Expr::Number(site as f64);
    }
}
