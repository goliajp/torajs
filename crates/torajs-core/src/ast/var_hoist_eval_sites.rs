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
    /// The placeholders (`undefined;`) at the head of a text that bind
    /// a name there, by the placeholder's expression: one per block
    /// function that may take a §B.3.2.3 `var` (whether it does is the
    /// lift's verdict), and one per `var` of a text evaluated as an
    /// expression, whose own `var`s became assignments.
    /// [`mark_eval_var_sites`] reads the verdict off the name's flavour.
    pub heads: std::collections::HashMap<ExprId, String>,
    /// The `var` sites that declare a name for an eval's text without
    /// binding it: the text's own head binds it (`heads`).
    pub inert_sites: std::collections::HashSet<ExprId>,
}

/// The hidden slot (`script_global_fns`) of a function an eval's text
/// declares at its top level, when that text is the only one that
/// declares it. Its global binding is created when the text runs, as
/// a property over the slot; before that the name is unbound, and
/// after it the property is what the name reads and calls — the
/// program may have put anything there first. Not a Script's
/// function: that binds where its declaration sits, which a read in
/// the Script may precede. Not one several texts declare: only the
/// last declaration is kept.
pub fn eval_bound_fn_slot(ast: &Ast, name: &str) -> Option<String> {
    if ast.eval_fn_names.get(name) != Some(&ScriptGlobalVar::EvalFn)
        || ast.eval_global_names.multi_text_fns.contains(name)
    {
        return None;
    }
    Some(format!(
        "{}{name}",
        super::script_global_fns::SCRIPT_GLOBAL_FN_SLOT_PREFIX
    ))
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
/// A head placeholder becomes the same self-assignment, in place, when
/// its name is site-bound, and stays inert otherwise; a placeholder may
/// sit in a function body (the IIFE an expression-position eval
/// becomes), where the walk does not go.
pub(super) fn mark_eval_var_sites(top: &mut [Stmt], ast: &mut Ast) {
    let sites = std::mem::take(&mut ast.eval_var_sites);
    let mut heads: Vec<(ExprId, String)> = std::mem::take(&mut ast.eval_global_names.heads)
        .into_iter()
        .collect();
    heads.sort_unstable_by_key(|(e, _)| e.0);
    for (mark, name) in heads {
        if let Some(flavor @ (ScriptGlobalVar::EvalVar | ScriptGlobalVar::ScriptVar)) =
            ast.script_global_var_names.get(&name).copied()
        {
            let target = ast.add_expr(Expr::Ident(name.clone()));
            let value = ast.add_expr(Expr::Ident(name));
            ast.eval_var_sites.insert(value, flavor);
            ast.exprs[mark.0 as usize] = Expr::Assign { target, value };
        }
    }
    for s in top.iter_mut() {
        mark_stmt(s, ast, &sites);
    }
}

fn mark_stmt(
    s: &mut Stmt,
    ast: &mut Ast,
    sites: &std::collections::HashMap<ExprId, ScriptGlobalVar>,
) {
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
            && !ast.eval_global_names.inert_sites.contains(init)
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
        mark_stmt(c, ast, sites);
    }
}
