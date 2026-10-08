//! The parameters of the any-widened clones that hold a value of any
//! kind where their original declares a typed slot.
//!
//! A twin (`<f>$$anyws`) or a widen clone (`<f>$$anyw<slots>`) binds
//! each widened argument as passed (§10.2.1.3): `const f: any = (x:
//! number) => { n = x }; f("q")` runs the closure's twin, whose `x`
//! holds "q". A binding such a parameter is stored into holds it as is
//! too (§13.15.2), so the parameter is a source for `hidden_any`.

use crate::ast::{Ast, Param, Stmt};

/// `(clone, parameter)` for every parameter an any-widened clone spells
/// `any` while the same position of its original does not.
pub(super) fn widened_params(ast: &Ast) -> Vec<(String, String)> {
    let decls: Vec<(&str, &[Param])> = (ast.stmts.iter())
        .filter_map(|s| match s {
            Stmt::FnDecl { name, params, .. } => Some((name.as_str(), params.as_slice())),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for &(name, params) in &decls {
        let Some((orig, _)) = name.rsplit_once("$$anyw") else {
            continue;
        };
        let Some(&(_, orig_params)) = decls.iter().find(|(n, _)| *n == orig) else {
            continue;
        };
        if orig_params.len() != params.len() {
            continue;
        }
        for (p, o) in params.iter().zip(orig_params) {
            if p.type_ann.as_deref() == Some("any") && o.type_ann.as_deref() != Some("any") {
                out.push((name.to_string(), p.name.clone()));
            }
        }
    }
    out
}
