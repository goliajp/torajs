//! RFC 20260810-sloppy-goal-arguments S2 — the sloppy goal's
//! callee-value machinery for
//! [`super::arguments_object::desugar_arguments_object`]: the shim
//! ledger, the per-fn value spelling, and the zero-capture closure
//! collection. Split to a sibling when the S2 knives pushed the main
//! pass past the 500-line file limit (verbatim move).

use super::{Ast, Expr, Stmt};

/// Zero-capture closure-shaped fns (lifted fn expressions whose
/// `__env()` annotation carries no captures): their callee value is
/// `Closure { name, [] }` behind a `__calleeval_` wrapper — a
/// capturing one has no expressible self-value (its env contents are
/// not reachable as bindings; recorded L3b window).
pub(super) fn collect_zero_env_closures(ast: &Ast) -> std::collections::HashSet<String> {
    ast.stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, params, .. }
                if params.first().is_some_and(|p| {
                    p.name == "__env" && p.type_ann.as_deref() == Some("__env()")
                }) =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect()
}

/// S2 — the sloppy callee-value shim ledger: decls to append after
/// the rewrite loop, the `__forward_` names already present (or
/// pending), and the fn-signature snapshot the synthesizer reads.
pub(super) struct PendingFwds {
    pub(super) decls: Vec<Stmt>,
    pub(super) known: std::collections::HashSet<String>,
    pub(super) sigs:
        std::collections::HashMap<String, (Vec<super::Param>, Option<String>, crate::lexer::Span)>,
    pub(super) zero_env: std::collections::HashSet<String>,
}

/// How this fn's callee VALUE spells (see [`SloppyCallee`] — the
/// same two shapes, owned).
pub(super) enum CalleeSpelling {
    /// `Closure { <shim>, [] }` — the plain fn's `__forward_` shim.
    Shim(String),
    /// `<wrapper>()` — a call of the `__calleeval_` wrapper minting
    /// `Closure { fn, [] }` OUTSIDE the fn's own body (the
    /// self-referential literal recursed the checker's walk).
    EvalCall(String),
}

/// The spelling of this fn's callee VALUE, if one is expressible: a
/// plain fn rides its `__forward_` shim (synthesized on demand via
/// the fn-to-closure pass's builder); a zero-capture closure-shaped
/// fn gets a `__calleeval_` wrapper returning `Closure { name, [] }`;
/// a capturing closure has none — its env contents are not reachable
/// as bindings at the mint (recorded sloppy-goal window, plan-state
/// L3b: the materialized body still rides Keyed, so writes / deletes
/// work and only the un-redefined read answers undefined).
pub(super) fn callee_value_fn(
    ast: &mut Ast,
    name: &str,
    pf: &mut PendingFwds,
) -> Option<CalleeSpelling> {
    if pf.zero_env.contains(name) {
        let wrapper = format!("__calleeval_{name}");
        if !pf.known.contains(&wrapper) {
            let cl = ast.add_expr(Expr::Closure {
                fn_name: name.to_string(),
                captures: Vec::new(),
            });
            pf.decls.push(Stmt::FnDecl {
                name: wrapper.clone(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: Some("any".into()),
                body: vec![Stmt::Return(Some(cl))],
                is_generator: false,
                span: crate::lexer::Span { start: 0, end: 0 },
            });
            pf.known.insert(wrapper.clone());
        }
        return Some(CalleeSpelling::EvalCall(wrapper));
    }
    let fwd = format!("__forward_{name}");
    if pf.known.contains(&fwd) {
        return Some(CalleeSpelling::Shim(fwd));
    }
    if !pf.sigs.contains_key(name) {
        return None;
    }
    let mut targets = std::collections::HashSet::new();
    targets.insert(name.to_string());
    let (decls, _renames) = super::forwarders_object::synthesize_forwarder_decls(
        ast,
        &targets,
        &pf.sigs,
        &std::collections::HashSet::new(),
    );
    pf.decls.extend(decls);
    pf.known.insert(fwd.clone());
    Some(CalleeSpelling::Shim(fwd))
}
