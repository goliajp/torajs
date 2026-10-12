//! The block references of an Annex B block function that the lift
//! wrote a var binding for.
//!
//! §B.3.3 gives a block function in sloppy code two bindings: the block
//! one, initialized on block entry (§14.2.3), and a var one written where
//! the declaration's text sits. The lift mints the write
//! (`var f = <lifted>`, or `f = <lifted>` when the scope binds `f`
//! already) and leaves every reference on the original name, which is
//! right outside the block and wrong inside it: a call written above the
//! declaration read the var binding before the write and threw
//! `TypeError` where the block binding is already the function. Inside
//! the block the references mean the block binding, whose value is the
//! lifted function itself, so they take its name — unless the block
//! assigns the name. Then the block binding is a variable of its own:
//! `{ f(); function f() {} f = null; }` in a loop gets a fresh binding
//! holding the function on every entry, so it is minted as a `let` at
//! the block head, the references take the `let`'s name, and the
//! §B.3.3 write copies the `let`'s value at the declaration's text
//! (§B.3.3.1: `benv.GetBindingValue(F)`).

use super::super::desugar_with::walk::stmt_children_ref;
use super::super::free_vars_write::assigns_name;
use super::super::nested_fns_idents::rewrite_idents_in_stmt;
use super::super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashMap;

/// Every statement list nested under `body` (not `body` itself, whose
/// writes are body-level hoists, not block ones).
pub(in crate::ast) fn redirect_block_refs(ast: &mut Ast, body: &mut [Stmt]) {
    for s in body.iter_mut() {
        nested(ast, s);
    }
}

fn nested(ast: &mut Ast, s: &mut Stmt) {
    match s {
        Stmt::Block(v) => list(ast, v),
        Stmt::Multi(v) => redirect_block_refs(ast, v),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            nested(ast, then_branch);
            if let Some(e) = else_branch {
                nested(ast, e);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => nested(ast, body),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            list(ast, body);
            list(ast, catch_body);
            if let Some(f) = finally_body {
                list(ast, f);
            }
        }
        _ => {}
    }
}

fn list(ast: &mut Ast, stmts: &mut Vec<Stmt>) {
    redirect_block_refs(ast, stmts);
    let writes: Vec<(usize, String, String)> = stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| write_of(ast, s).map(|(n, m)| (i, n, m)))
        .collect();
    if writes.is_empty() {
        return;
    }
    let mut map: HashMap<String, String> = HashMap::new();
    let mut heads: Vec<Stmt> = Vec::new();
    for (w, name, lifted) in &writes {
        let assigned = stmts
            .iter()
            .enumerate()
            .any(|(i, s)| !writes.iter().any(|(x, _, _)| x == &i) && assigns_name(ast, s, name));
        if !assigned {
            map.insert(name.clone(), lifted.clone());
            continue;
        }
        let local = format!("__blkfn{lifted}");
        ast.renamed_binding_display
            .insert(local.clone(), name.clone());
        let init = ast.add_expr(Expr::Ident(lifted.clone()));
        heads.push(Stmt::LetDecl {
            mutable: true,
            name: local.clone(),
            type_ann: Some("any".to_string()),
            init,
            is_var: false,
        });
        if let Some(value) = write_value(ast, &stmts[*w]) {
            ast.exprs[value.0 as usize] = Expr::Ident(local.clone());
        }
        map.insert(name.clone(), local);
    }
    let skip: Vec<usize> = writes.iter().map(|(w, _, _)| *w).collect();
    for (i, s) in stmts.iter_mut().enumerate() {
        if skip.contains(&i) {
            continue;
        }
        // a nested block declaring the same function again owns it there
        let own: HashMap<String, String> = map
            .iter()
            .filter(|(n, _)| !writes_under(ast, s, n))
            .map(|(n, m)| (n.clone(), m.clone()))
            .collect();
        rewrite_idents_in_stmt(ast, s, &own, true);
    }
    stmts.splice(0..0, heads);
}

fn write_value(ast: &Ast, s: &Stmt) -> Option<ExprId> {
    match s {
        Stmt::LetDecl { init, .. } => Some(*init),
        Stmt::Expr(e) => match ast.get_expr(*e) {
            Expr::Assign { value, .. } => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

fn writes_under(ast: &Ast, s: &Stmt, name: &str) -> bool {
    write_of(ast, s).is_some_and(|(n, _)| n == name)
        || stmt_children_ref(s)
            .into_iter()
            .any(|c| writes_under(ast, c, name))
}

/// `var f = <lifted f>` or `f = <lifted f>`: the §B.3.3 write.
fn write_of(ast: &Ast, s: &Stmt) -> Option<(String, String)> {
    let (name, value): (&str, ExprId) = match s {
        Stmt::LetDecl {
            name,
            init,
            is_var: true,
            ..
        } => (name, *init),
        Stmt::Expr(e) => match ast.get_expr(*e) {
            Expr::Assign { target, value } => match ast.get_expr(*target) {
                Expr::Ident(n) => (n, *value),
                _ => return None,
            },
            _ => return None,
        },
        _ => return None,
    };
    let Expr::Ident(lifted) = ast.get_expr(value) else {
        return None;
    };
    // a write `list` already pointed at the block's own `let`
    let lifted = lifted.strip_prefix("__blkfn").unwrap_or(lifted);
    (lifted.starts_with("__nested_") && ast.nested_fn_names.get(lifted).is_some_and(|n| n == name))
        .then(|| (name.to_string(), lifted.to_string()))
}
