//! The top-level seed of the implicit-generics signature map.

use crate::ast::Stmt;

/// Chunk 522 — run the `__closure_*` param-default + return-sniff
/// branches ahead of the main loop (idempotent with it), then
/// publish each closure's full fn-shaped ann into `fn_sigs` under
/// its reserved name. Closures whose value returns resisted typing
/// publish nothing (no fabricated ann); a body without value
/// returns is `void`.
/// Top-level seed of the signature map — every non-generic user
/// `FnDecl` that SPELLS its return type. Closure bodies are excluded
/// here on purpose: `preinfer_closure_sigs` publishes theirs under
/// the reserved `__closure_*` names once the lifted decls exist.
pub(super) fn seed_declared_fn_sigs(stmts: &[Stmt]) -> std::collections::HashMap<String, String> {
    let mut fn_sigs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for s in stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type: Some(rt),
            type_params,
            ..
        } = s
            && !name.starts_with("__closure_")
            && type_params.is_empty()
        {
            // A `__forward_<fn>` shim is a closure VALUE (rotation 646):
            // the map's bare return spelling is what a CALL of the
            // name answers, but the infer's `Expr::Closure` arm reads
            // this entry as the value's own type — `{ next: sf }`
            // returned from an arrow inferred `__inlobj(next:number)`
            // and the checker refused the literal. Its params are the
            // target's, cloned with their annotations, so the
            // fn-shaped sig `preinfer_closure_sigs` would publish later
            // is spellable here, ahead of the first closure that reads
            // it.
            if crate::ast::is_shell(name) {
                let mut param_anns: Vec<String> = Vec::with_capacity(params.len());
                let complete = params.iter().filter(|p| p.name != "__env").all(|p| {
                    p.type_ann.as_ref().is_some_and(|a| {
                        param_anns.push(a.clone());
                        true
                    })
                });
                if complete {
                    fn_sigs.insert(
                        name.clone(),
                        crate::type_ann_fnsig::fn_type_ann("__fn", &param_anns.join("|"), rt),
                    );
                }
                continue;
            }
            fn_sigs.insert(name.clone(), rt.clone());
        }
    }
    fn_sigs
}
