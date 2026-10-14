//! §19.2.1.3 EvalDeclarationInstantiation (and §16.1.7 for a Script)
//! for global `var` code: every `var` the text declares, and every
//! block-nested function §B.3.2.3 gives a `var` binding, is bound on
//! the global object BEFORE the text's first statement runs — not
//! where the declaration sits in it. `eval('initial = x; var x;')`
//! reads `undefined`, and `eval('f; { function f() {} }')` finds the
//! property `f` already there.
//!
//! The binding is site-bound (`var_hoist_eval_sites`): it happens at a
//! write of the slot. So the head of the inlined text gets an init-less
//! `var <name>;` per name, which the hoist turns into the binding
//! self-assignment; the declarations in the text bind again later, over
//! a property that is already the binding, and change nothing.
//!
//! A block function's binding is created only where §B.3.2.3 creates
//! it, and that is decided later, by the lift (`annexb_fn_var`), which
//! must not see a `var` of the name it did not write — a `var f` in the
//! scope changes what the block function's own references to `f`
//! mean. So the head of the text gets an inert placeholder per
//! candidate name instead (`EvalGlobalNames::heads`), which the
//! `var` hoist turns into the binding self-assignment only when the
//! lift did give the name its `var` (`var_hoist_eval_sites`). A name a
//! lexical declaration of the program, or of a scope of the text that
//! encloses the function, binds is not a candidate.

use std::collections::{HashMap, HashSet};

use super::super::desugar_with::walk::stmt_children_ref;
use super::super::{Ast, Expr, Stmt};

#[derive(Default)]
struct TextNames {
    /// Function declarations at the text's own top level.
    top_fns: HashSet<String>,
    /// Block-nested plain function declarations that §B.3.2.3 may give
    /// a `var`, in textual order: replacing one with `var F` would not
    /// collide with a lexical declaration of `F` in a scope enclosing
    /// it, the text's top level included. A catch parameter is not
    /// such a declaration (§B.3.5), and a sibling block's is not in
    /// the way.
    block_fns: Vec<String>,
}

/// Prepend the instantiation `var`s to an inlined global text.
pub(super) fn prepend_global_instantiation(inlined: &mut Vec<Stmt>, ast: &mut Ast) {
    let mut names = TextNames::default();
    scope_list(inlined, false, &mut Vec::new(), ast, &mut names);
    let mut counts: HashMap<String, Vec<super::super::ExprId>> = HashMap::new();
    super::super::var_hoist::count_var_names(inlined, &mut counts);
    let mut vars: Vec<(u32, String)> = counts
        .into_iter()
        .map(|(n, sites)| {
            (
                sites
                    .iter()
                    .map(|e| e.0)
                    .min()
                    .expect("a counted name has a site"),
                n,
            )
        })
        .collect();
    vars.sort();
    let mut order: Vec<(String, bool)> = vars.into_iter().map(|(_, n)| (n, false)).collect();
    let mut seen: HashSet<String> = order.iter().map(|(n, _)| n.clone()).collect();
    for n in names.block_fns {
        if seen.insert(n.clone()) {
            order.push((n, true));
        }
    }
    order.retain(|(n, _)| {
        !names.top_fns.contains(n)
            && !ast.script_lex_names.contains(n)
            && !ast.eval_global_names.program_lex.contains(n)
    });
    let head: Vec<Stmt> = order
        .into_iter()
        .map(|(name, block_fn)| {
            if block_fn {
                let mark = ast.add_expr(Expr::Ident("undefined".to_string()));
                ast.eval_global_names.heads.insert(mark, name);
                return Stmt::Expr(mark);
            }
            Stmt::LetDecl {
                mutable: true,
                name,
                type_ann: None,
                init: ast.add_expr(Expr::Uninit),
                is_var: true,
            }
        })
        .collect();
    inlined.splice(0..0, head);
}

/// One statement list, a scope of its own, with the lexical names of
/// the scopes around it in `encl`.
fn scope_list(
    stmts: &[Stmt],
    nested: bool,
    encl: &mut Vec<HashSet<String>>,
    ast: &Ast,
    out: &mut TextNames,
) {
    let mut here = HashSet::new();
    direct_lexical(stmts.iter(), &mut here);
    encl.push(here);
    for s in stmts {
        visit(s, nested, encl, ast, out);
    }
    encl.pop();
}

/// The lexical names a list declares itself; a `Multi` shares its
/// list's scope.
fn direct_lexical<'s>(stmts: impl Iterator<Item = &'s Stmt>, out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            }
            | Stmt::UsingDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::Multi(v) => direct_lexical(v.iter(), out),
            _ => {}
        }
    }
}

fn visit(s: &Stmt, nested: bool, encl: &mut Vec<HashSet<String>>, ast: &Ast, out: &mut TextNames) {
    match s {
        Stmt::FnDecl {
            name, is_generator, ..
        } => {
            if !nested {
                out.top_fns.insert(name.clone());
            } else if !*is_generator
                && !ast.async_fns.contains(name)
                && !ast.async_generator_fns.contains(name)
                && !encl.iter().any(|h| h.contains(name))
            {
                out.block_fns.push(name.clone());
            }
        }
        Stmt::ClassDecl { .. } => {}
        Stmt::Multi(v) => {
            for c in v {
                visit(c, nested, encl, ast, out);
            }
        }
        Stmt::Block(b) => scope_list(b, true, encl, ast, out),
        Stmt::Switch { cases, default, .. } => {
            // The clauses share one block.
            let all: Vec<&Stmt> = cases
                .iter()
                .flat_map(|c| c.body.iter())
                .chain(default.iter().flatten())
                .collect();
            let mut here = HashSet::new();
            direct_lexical(all.iter().copied(), &mut here);
            encl.push(here);
            for c in all {
                visit(c, true, encl, ast, out);
            }
            encl.pop();
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            scope_list(body, true, encl, ast, out);
            scope_list(catch_body, true, encl, ast, out);
            if let Some(f) = finally_body {
                scope_list(f, true, encl, ast, out);
            }
        }
        _ => {
            // A loop head's `let`, or the binding of `for (… of …)`,
            // scopes the statement's body.
            let mut head = HashSet::new();
            match s {
                Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                    head.insert(var_name.clone());
                }
                Stmt::For { init: Some(i), .. } => {
                    direct_lexical(std::iter::once(i.as_ref()), &mut head);
                }
                _ => {}
            }
            encl.push(head);
            for c in stmt_children_ref(s) {
                visit(c, true, encl, ast, out);
            }
            encl.pop();
        }
    }
}
