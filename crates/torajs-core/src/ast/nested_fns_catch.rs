//! Applying the catch-parameter renames the nested-fn lift recorded.
//!
//! §B.3.3.1 writes the block function's value with
//! `varEnv.SetMutableBinding` — the VariableEnvironment of the
//! enclosing function or script. A catch parameter is not part of that
//! environment, so a `catch (f)` around a block that declares
//! `function f` does not intercept the write: after the block, the
//! function is visible OUTSIDE the try, and inside the catch block `f`
//! is still the caught value.
//!
//! tr lowers a catch parameter as a scope of its own wrapped around the
//! catch block, so a write left under the original name would land on
//! the parameter. The lift renames the parameter; this pass rewrites
//! the references that mean it. Only the catch block is touched — the
//! references outside it are the var binding, which is the point.
//! `Stmt::LetDecl`'s name is a field rather than an expression, so the
//! §B.3.3 write minted under the original name is left alone by
//! construction.

use super::annexb_fn_var::LiftCtx;
use super::nested_fns_idents::rewrite_idents_in_body;
use super::{Ast, Stmt};
use std::collections::HashMap;

/// Rename a catch parameter that would intercept a §B.3.3 write, and
/// record the pair for [`apply_catch_renames`]. The write itself keeps
/// the original name, which is how it reaches the var binding.
pub(super) fn rename_catch_param_if_shadowing(
    catch_param: &mut Option<String>,
    catch_body: &[Stmt],
    ctx: &mut LiftCtx,
) {
    let Some(p) = catch_param else { return };
    if !ctx.catch_param_shadows_write(p, catch_body) {
        return;
    }
    let fresh = format!("__catch_{p}_{}", ctx.counter);
    ctx.counter += 1;
    ctx.catch_renames.push((p.clone(), fresh.clone()));
    *p = fresh;
}

pub(super) fn apply_catch_renames(ast: &mut Ast, body: &mut [Stmt], pairs: &[(String, String)]) {
    if pairs.is_empty() {
        return;
    }
    for s in body.iter_mut() {
        apply_in_stmt(ast, s, pairs);
    }
}

fn apply_in_stmt(ast: &mut Ast, stmt: &mut Stmt, pairs: &[(String, String)]) {
    match stmt {
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            apply_catch_renames(ast, body, pairs);
            // The fresh name is unique across the pass, so matching the
            // parameter finds exactly the catch the walk renamed.
            if let Some(pair) = catch_param
                .as_deref()
                .and_then(|p| pairs.iter().find(|(_, fresh)| fresh == p))
            {
                let map: HashMap<String, String> =
                    HashMap::from([(pair.0.clone(), pair.1.clone())]);
                rewrite_idents_in_body(ast, catch_body, &map, true);
            }
            apply_catch_renames(ast, catch_body, pairs);
            if let Some(fb) = finally_body {
                apply_catch_renames(ast, fb, pairs);
            }
        }
        Stmt::Block(b) | Stmt::Multi(b) => apply_catch_renames(ast, b, pairs),
        Stmt::FnDecl { body, .. } => apply_catch_renames(ast, body, pairs),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            apply_in_stmt(ast, then_branch, pairs);
            if let Some(eb) = else_branch.as_deref_mut() {
                apply_in_stmt(ast, eb, pairs);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => apply_in_stmt(ast, body, pairs),
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                apply_catch_renames(ast, &mut c.body, pairs);
            }
            if let Some(d) = default {
                apply_catch_renames(ast, d, pairs);
            }
        }
        _ => {}
    }
}
