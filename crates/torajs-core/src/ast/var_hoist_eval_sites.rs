//! RFC 20260912-global-var-cell knife 4 — the `var` sites that came
//! out of eval / Script text. Two questions the script-goal block of
//! `desugar_var_hoist` asks about them:
//!
//! - which flavour does a name take? A name EVERY site of which is
//!   eval / Script text is that text's (`EvalVar`: deletable,
//!   §19.2.1.3; `ScriptVar`: not, §16.1.7). A name the program wrote
//!   anywhere is a written `var` — the property exists from program
//!   start whatever the text later re-declares.
//! - where does it bind? Where the text ran, not at program start
//!   (`(0, eval)("var x")` after `Object.defineProperty(this, "x", …)`
//!   must find the property and keep it — §16.1.7 CanDeclareGlobalVar
//!   / §19.2.1.3 — and `"x" in globalThis` before the eval is false).
//!   The bind emitter binds these flavours at a WRITE of the slot; a
//!   `var x = 1` site is one after the hoist, an init-less `var x;`
//!   site would be removed, so it is made a self-assignment `x = x`
//!   first — a store the hoist turns into the write the emitter binds
//!   on, and one that changes no value.

use super::desugar_with::walk;
use super::var_hoist::ScriptGlobalVar;
use super::{Ast, Expr, ExprId, Stmt};

/// What the global instantiation of an eval / Script text needs to
/// know about the rest of the program (`desugar_eval::instantiate`,
/// `ssa_lower_ident_site_bound`). Filed by the collision pre-pass,
/// which sees the program as written.
#[derive(Debug, Clone, Default)]
pub struct EvalGlobalNames {
    /// The program's own top-level lexical names. A block function of
    /// the text whose name is one of these gets no §B.3.2.3 binding.
    pub program_lex: std::collections::HashSet<String>,
    /// Functions more than one text declares. Only the last
    /// declaration is kept (`keep_last_fn_decls`), so the property
    /// binds there, and an earlier text's reads cannot be told apart
    /// from reads before any text ran.
    pub multi_text_fns: std::collections::HashSet<String>,
    /// The placeholders (`undefined;`) at the head of a text, one per
    /// block function that may take a §B.3.2.3 `var`, by the
    /// placeholder's expression. Whether it does is the lift's verdict;
    /// [`mark_eval_var_sites`] reads it off the name's flavour.
    pub annexb_heads: std::collections::HashMap<ExprId, String>,
}

/// The flavour a name's sites give it, or `None` when some site is
/// the program's own.
pub(super) fn eval_flavor(
    sites: &[ExprId],
    eval_sites: &std::collections::HashMap<ExprId, ScriptGlobalVar>,
) -> Option<ScriptGlobalVar> {
    if sites.is_empty() || !sites.iter().all(|s| eval_sites.contains_key(s)) {
        return None;
    }
    if sites
        .iter()
        .any(|s| eval_sites.get(s) == Some(&ScriptGlobalVar::ScriptVar))
    {
        Some(ScriptGlobalVar::ScriptVar)
    } else {
        Some(ScriptGlobalVar::EvalVar)
    }
}

/// Give every init-less eval / Script `var` site of a site-bound name
/// the self-assignment init described in the module doc. Walks the
/// positions the hoist walks (not into function bodies).
///
/// The flavours are decided by now, so `Ast::eval_var_sites` is left
/// holding the minted self-reads alone: the read of `x` in `x = x`
/// runs before the property exists — that store is what creates it —
/// and must not raise the ReferenceError every other read of a name
/// not yet bound raises (`ssa_lower_ident_site_bound`).
///
/// A block function's head placeholder becomes the same
/// self-assignment when the lift gave the name a site-bound `var`, and
/// stays inert otherwise.
pub(super) fn mark_eval_var_sites(top: &mut [Stmt], ast: &mut Ast) {
    let sites = std::mem::take(&mut ast.eval_var_sites);
    let heads = std::mem::take(&mut ast.eval_global_names.annexb_heads);
    for s in top.iter_mut() {
        mark_stmt(s, ast, &sites, &heads);
    }
}

fn mark_stmt(
    s: &mut Stmt,
    ast: &mut Ast,
    sites: &std::collections::HashMap<ExprId, ScriptGlobalVar>,
    heads: &std::collections::HashMap<ExprId, String>,
) {
    if let Stmt::Expr(mark) = s
        && let Some(name) = heads.get(mark)
        && let Some(flavor @ (ScriptGlobalVar::EvalVar | ScriptGlobalVar::ScriptVar)) =
            ast.script_global_var_names.get(name).copied()
    {
        let target = ast.add_expr(Expr::Ident(name.clone()));
        let value = ast.add_expr(Expr::Ident(name.clone()));
        ast.eval_var_sites.insert(value, flavor);
        *mark = ast.add_expr(Expr::Assign { target, value });
        return;
    }
    if let Stmt::LetDecl {
        name,
        init,
        is_var: true,
        ..
    } = s
    {
        let site_bound = matches!(
            ast.script_global_var_names.get(name),
            Some(ScriptGlobalVar::EvalVar | ScriptGlobalVar::ScriptVar)
        );
        if site_bound
            && let Some(flavor) = sites.get(init)
            && matches!(ast.exprs[init.0 as usize], Expr::Uninit)
        {
            *init = ast.add_expr(Expr::Ident(name.clone()));
            ast.eval_var_sites.insert(*init, *flavor);
        }
        return;
    }
    if matches!(s, Stmt::FnDecl { .. }) {
        return;
    }
    for c in walk::stmt_children(s) {
        mark_stmt(c, ast, sites, heads);
    }
}
