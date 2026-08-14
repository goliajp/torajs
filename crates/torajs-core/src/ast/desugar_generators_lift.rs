//! 393-03 — nested `function*` declarations (inside a block or a
//! fn body) lift to the top level BEFORE the generator desugar
//! claims its decls: the claim walk (`collect_generator_fn_decls`)
//! reads top-level FnDecls only, so an unlifted nested generator
//! kept its `yield` and died loud at check ("yield is only valid
//! inside a `function*` generator body").
//!
//! Why not the general `desugar_nested_fns` pass, earlier: its
//! pipeline position AFTER `desugar_capturing_nested_fns` is
//! load-bearing — running it before the capturing lane would lift
//! (and so break) every capturing nested fn. Generators never reach
//! that stage at all (the desugar in the parse-time pipeline
//! consumes them), so a generator-only lift here competes with no
//! one. A nested generator that CAPTURES an enclosing local still
//! fails loud after the lift (unknown identifier at the top-level
//! body) — same waterline as before, now past the yield gate.

use std::collections::HashMap;

use super::nested_fns_idents::{rewrite_idents_in_body, rewrite_idents_in_stmt};
use super::stmt_nested_lists::for_each_nested_vec_mut;
use super::{Ast, Stmt};

/// Lift every nested `is_generator` FnDecl to the top level under a
/// mangled name (`__nestedgen_<parent>_<name>_<uid>`), rewriting the
/// parent's references and the lifted body's own (self-recursion).
/// The original site becomes an empty Block. Block-scoped semantics
/// are the plain ES2015 ones — no Annex B branch carve-outs apply
/// to generator declarations.
pub(super) fn lift_nested_generators(ast: &mut Ast) {
    let mut counter = 0u32;
    // Cloned up front — the collect closure below runs while `ast`
    // is also needed mutably by the rewrite calls right after it.
    let ast_async_gens = ast.async_generator_fns.clone();
    let mut top = std::mem::take(&mut ast.stmts);
    let mut all_lifted: Vec<Stmt> = Vec::new();
    for stmt in top.iter_mut() {
        let parent = match stmt {
            Stmt::FnDecl { name, .. } => name.clone(),
            _ => "__top".to_string(),
        };
        let mut renames: HashMap<String, String> = HashMap::new();
        let mut lifted: Vec<Stmt> = Vec::new();
        for_each_nested_vec_mut(stmt, &mut |list| {
            for slot in list.iter_mut() {
                let Stmt::FnDecl {
                    name,
                    is_generator: true,
                    ..
                } = slot
                else {
                    continue;
                };
                if name.starts_with("__nestedgen_") {
                    continue;
                }
                // A nested ASYNC generator stays put: its channel
                // (the parser's `async_generator_fns` registry + the
                // async desugar's drive) is keyed by the original
                // name and does not survive the rename — lifting it
                // turned the pre-existing loud compile error into a
                // SIGSEGV (A/B probed). It keeps today's loud reject.
                if ast_async_gens.contains(name.as_str()) {
                    continue;
                }
                let mangled = format!("__nestedgen_{parent}_{name}_{counter}");
                counter += 1;
                renames.insert(name.clone(), mangled.clone());
                let taken = std::mem::replace(slot, Stmt::Block(Vec::new()));
                let Stmt::FnDecl {
                    type_params,
                    params,
                    return_type,
                    body,
                    span,
                    ..
                } = taken
                else {
                    unreachable!();
                };
                lifted.push(Stmt::FnDecl {
                    name: mangled,
                    type_params,
                    params,
                    return_type,
                    body,
                    is_generator: true,
                    span,
                });
            }
        });
        if !renames.is_empty() {
            // The stmt rewriter has no FnDecl arm (the general
            // nested-fns pass hands FnDecl BODIES to the body
            // rewriter separately) — mirror that split here.
            match &mut *stmt {
                Stmt::FnDecl { body, .. } => rewrite_idents_in_body(ast, body, &renames, true),
                other => rewrite_idents_in_stmt(ast, other, &renames, true),
            }
            remap_parser_gen_anns(stmt, &renames);
            for lf in lifted.iter_mut() {
                if let Stmt::FnDecl { body, .. } = lf {
                    rewrite_idents_in_body(ast, body, &renames, true);
                    remap_parser_gen_anns(lf, &renames);
                }
            }
        }
        all_lifted.extend(lifted);
    }
    top.extend(all_lifted);
    ast.stmts = top;
}

/// The parser's for-of fast path bakes `__Gen_<name>` /
/// `__step_<name>` annotations at PARSE time, spelling the original
/// generator name — remap them alongside the ident references, or
/// the hoisted iterator binding points at a class the (post-lift)
/// desugar never mints ("unknown type `__Gen_inner`").
fn remap_parser_gen_anns(s: &mut Stmt, renames: &HashMap<String, String>) {
    for_each_nested_vec_mut(s, &mut |list| {
        for st in list.iter_mut() {
            let Stmt::LetDecl {
                type_ann: Some(ann),
                ..
            } = st
            else {
                continue;
            };
            for (old, new) in renames {
                if *ann == format!("__Gen_{old}") {
                    *ann = format!("__Gen_{new}");
                } else if *ann == format!("__step_{old}") {
                    *ann = format!("__step_{new}");
                }
            }
        }
    });
}
