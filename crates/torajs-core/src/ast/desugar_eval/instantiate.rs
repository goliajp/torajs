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
//! candidate name instead (`EvalGlobalNames::annexb_heads`), which the
//! `var` hoist turns into the binding self-assignment only when the
//! lift did give the name its `var` (`var_hoist_eval_sites`). A name a
//! lexical declaration in the text or the program also binds is not a
//! candidate.

use std::collections::{HashMap, HashSet};

use super::super::desugar_with::walk::stmt_children_ref;
use super::super::{Ast, Expr, Stmt};

#[derive(Default)]
struct TextNames {
    /// Function declarations at the text's own top level.
    top_fns: HashSet<String>,
    /// Block-nested plain function declarations, in textual order.
    block_fns: Vec<String>,
    /// Every name a lexical declaration or a `for (… of / in …)` head
    /// binds anywhere in the text. A catch parameter is not one: it
    /// does not block the §B.3.3 binding (§B.3.5).
    lexical: HashSet<String>,
}

/// Prepend the instantiation `var`s to an inlined global text.
pub(super) fn prepend_global_instantiation(inlined: &mut Vec<Stmt>, ast: &mut Ast) {
    let mut names = TextNames::default();
    for s in inlined.iter() {
        collect(s, false, ast, &mut names);
    }
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
        if !names.lexical.contains(&n) && seen.insert(n.clone()) {
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
                ast.eval_global_names.annexb_heads.insert(mark, name);
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

fn collect(s: &Stmt, nested: bool, ast: &Ast, out: &mut TextNames) {
    match s {
        Stmt::FnDecl {
            name, is_generator, ..
        } => {
            if !nested {
                out.top_fns.insert(name.clone());
            } else if !*is_generator
                && !ast.async_fns.contains(name)
                && !ast.async_generator_fns.contains(name)
            {
                out.block_fns.push(name.clone());
            }
            return;
        }
        Stmt::LetDecl {
            name,
            is_var: false,
            ..
        }
        | Stmt::UsingDecl { name, .. } => {
            out.lexical.insert(name.clone());
        }
        Stmt::ClassDecl { name, .. } => {
            out.lexical.insert(name.clone());
            return;
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            out.lexical.insert(var_name.clone());
        }
        _ => {}
    }
    // A `Multi` shares its list's scope: its children are as nested as
    // the list itself is.
    let child_nested = nested || !matches!(s, Stmt::Multi(_));
    for c in stmt_children_ref(s) {
        collect(c, child_nested, ast, out);
    }
}
