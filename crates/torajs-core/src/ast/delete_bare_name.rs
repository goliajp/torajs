//! `delete <bare name>` goal triage — ES §13.5.1 (rotation 372).
//!
//! The parser can no longer judge this form: §13.5.1.1 makes it a
//! SyntaxError under the STRICT goal only, and the goal bit
//! (`ast.sloppy_script_goal`, keyed on the `.cts` extension per the
//! bun mapping) is stamped after parsing. So the parser now emits a
//! plain `Delete { Ident }` node and this raw-AST gate — run in the
//! prelude right after the redeclaration early errors — resolves it
//! per goal:
//!
//! - **strict** (every `.ts` module): the same SyntaxError text the
//!   parser used to raise.
//! - **sloppy** (`.cts`): §13.5.1.2 evaluates the reference —
//!   statically decidable here. A name declared anywhere in the
//!   program is a var/function/lexical binding, and §9.1.1.1.7
//!   DeleteBinding answers **false** for them (bindings minted by
//!   declarations are non-configurable). The non-configurable
//!   global value properties (`undefined` / `NaN` / `Infinity` /
//!   `globalThis`, §19.1) answer **false** too. Everything else —
//!   an unresolvable name (§13.5.1.2 step 3.a) or a configurable
//!   global builtin — answers **true**. RESIDUE (recorded): a
//!   configurable builtin answers true but is NOT actually removed
//!   from the global; a program that deletes `Math` and then reads
//!   it observes the difference.
//!
//! Both goals leave zero `Delete { Ident }` nodes behind, so no
//! downstream pass ever meets the shape.

use super::{Ast, Expr, Stmt};

/// Resolve every `delete <bare name>` site per the compile goal.
/// `Some(msg)` = strict-goal SyntaxError (the caller reports it as
/// a parse error and stops).
pub fn triage_delete_bare_names(ast: &mut Ast) -> Option<String> {
    let sites: Vec<(usize, String)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            if let Expr::Delete { expr } = e
                && let Expr::Ident(n) = ast.get_expr(*expr)
            {
                Some((i, n.clone()))
            } else {
                None
            }
        })
        .collect();
    if sites.is_empty() {
        return None;
    }
    if !ast.sloppy_script_goal {
        return Some(format!(
            "`delete` on a bare name is a SyntaxError in strict code (modules are strict) — `delete {}`",
            sites[0].1
        ));
    }
    let mut declared: std::collections::HashSet<String> = std::collections::HashSet::new();
    crate::ast_collect_bindings::collect_local_binding_names(&ast.stmts, &mut declared);
    collect_decl_names(&ast.stmts, &mut declared);
    for (i, n) in sites {
        let non_configurable = declared.contains(&n)
            || matches!(n.as_str(), "undefined" | "NaN" | "Infinity" | "globalThis");
        ast.exprs[i] = Expr::Bool(!non_configurable);
    }
    None
}

/// FnDecl / ClassDecl NAMES (the shared binding collector gathers
/// params and let/var/for-of/catch bindings but not the declaring
/// names themselves).
fn collect_decl_names(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::FnDecl { name, body, .. } => {
                out.insert(name.clone());
                collect_decl_names(body, out);
            }
            Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_decl_names(inner, out),
            _ => {}
        }
    }
}
