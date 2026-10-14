//! The binding of a class declared in a statement list that runs more
//! than once.
//!
//! §14.2.2 gives each pass of a loop body a fresh environment, and
//! §15.7.14 binds the class there, so a closure made on one pass
//! constructs that pass's class, whose members read that pass's
//! bindings. The lift keeps a generator-local class in one
//! state-machine field (`desugar_generators_prep`), which every
//! closure would then share; a closure reading the class instead reads
//! a per-pass local, as it does for a block `let`. A class binding is
//! never boxed: the closures that assign it are left alone.

use super::block_let::host;
use super::class_member::class_reads;
use super::scan::{arrow_reads, arrows_in, assigned, decl_reads, stmt_roots};
use super::{Cell, LiftCtx, unhosted};
use crate::ast::free_vars::top_ref_sites;
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Give the class `name` declared right before `after` a per-pass
/// local for the closures under `after` that read it.
pub(super) fn box_class(
    ast: &mut Ast,
    before: &[Stmt],
    name: &str,
    after: Vec<&mut Stmt>,
    lifted: &[(String, String)],
    ctx: &mut LiftCtx,
) {
    let mut roots = Vec::new();
    for s in after.iter() {
        stmt_roots(s, &mut roots);
    }
    let read = arrows_in(ast, &roots)
        .iter()
        .any(|&a| arrow_reads(ast, a, name))
        || after.iter().any(|s| class_reads(ast, s, name));
    // A reference made before the declaration has no class to hold
    // yet, and a function declared after it, being hoisted, has no
    // statement of its own to take the local.
    if !read
        || before
            .iter()
            .any(|s| !top_ref_sites(ast, s, name, &[]).is_empty())
        || after
            .iter()
            .any(|s| decl_reads(ast, s, name) || unhosted(ast, s, name))
    {
        return;
    }
    // the references that resolve to this binding, past every scope
    // that declares the name again
    let sites: Vec<ExprId> = after
        .iter()
        .flat_map(|s| top_ref_sites(ast, s, name, &[]))
        .collect();
    let Some((_, ty)) = lifted.iter().find(|(n, _)| n == name) else {
        return;
    };
    if assigned(ast, &sites) {
        return;
    }
    let stem = format!("{name}__blk{}", ctx.cells);
    ctx.cells += 1;
    let marker = format!("{stem}__ref");
    for site in sites {
        match ast.get_expr(site).clone() {
            Expr::Ident(_) => ast.exprs[site.0 as usize] = Expr::Ident(marker.clone()),
            // the class as a value, as the lift constructs a lifted
            // class anyway
            Expr::New { args, .. } => {
                let callee = ast.add_expr(Expr::Ident(marker.clone()));
                ast.exprs[site.0 as usize] = Expr::NewDynamic { callee, args };
            }
            _ => {}
        }
    }
    let mut cell = Cell {
        field: name.to_string(),
        marker,
        ty: ty.clone(),
        stem,
        locals: 0,
        boxed: false,
    };
    host(ast, &mut cell, &roots, after, ctx);
}
