//! Forwarder-decl synthesis — the "what does one `__forward_<t>`
//! look like" half of [`super::forwarders_object`], split to a
//! sibling when the 402-01 generic-target erasure pushed that file
//! past the 500-line limit (the erasure itself now lives in the one
//! synthesizer, `forwarder_synth`). The parent keeps the store-site
//! collection / rewrite walk; this file builds the decls it asked
//! for.

use super::{Ast, Param, Stmt};

/// Synthesize one `__forward_<target>(__env, args...) { return
/// target(args...); }` closure-shaped FnDecl per unique target
/// (skipping any `synthesize_forwarders` already produced), returning
/// the decls plus the target → forwarder-name rename map (chunk 783
/// extraction — the member-assign axis pushed the caller past the
/// 200-line fn limit).
pub(super) fn synthesize_forwarder_decls(
    ast: &mut Ast,
    targets: &std::collections::HashSet<String>,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    existing_forwarders: &std::collections::HashSet<String>,
) -> (Vec<Stmt>, std::collections::HashMap<String, String>) {
    let mut new_decls: Vec<Stmt> = Vec::new();
    let mut renames: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for target in targets {
        let forward_name = format!("__forward_{target}");
        if existing_forwarders.contains(&forward_name) {
            renames.insert(target.clone(), forward_name);
            continue;
        }
        let (params, return_type, target_span) = fn_sigs.get(target).unwrap().clone();
        // 402-01 — a GENERIC target's TypeVars are erased to `any`
        // by the synthesizer itself (every face, rotation 650).
        // A recv-first public face was tried (rotation 366) and
        // REVERTED — see `ast/forwarder_synth.rs` for the shape and
        // for why the receiver channel can only move per route.
        new_decls.push(super::forwarder_synth::synth_forwarder_decl(
            ast,
            &forward_name,
            target,
            &params,
            return_type,
            target_span,
            crate::ast::ShellFace::Plain,
        ));
        renames.insert(target.clone(), forward_name);
    }
    (new_decls, renames)
}
