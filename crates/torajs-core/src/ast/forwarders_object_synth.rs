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
        // A declaration that already has a shell keeps it, whichever
        // face it wears: `promote_this_fn_values` mints the
        // receiver-first one for every `this`-reading declaration, and
        // minting a plain one beside it would give that declaration a
        // second cell -- so `globalThis.t === t` would answer false
        // for exactly the functions that read `this`.
        if let Some(existing) = crate::ast::shell_names(target)
            .into_iter()
            .find(|n| existing_forwarders.contains(n))
        {
            renames.insert(target.clone(), existing);
            continue;
        }
        let (params, return_type, target_span) = fn_sigs.get(target).unwrap().clone();
        // A `this`-reading STATIC's twin takes the receiver-first
        // face: the twin exists precisely to carry a receiver
        // (`__smany_`, knife 3a), and the plain face's whole job is to
        // pin one to `undefined` — so the runtime `.bind` kernel had
        // nowhere to put the bound this and `C.method.bind(C)` lost
        // it, which is twelve test262 cases in the async-private-static
        // family.
        //
        // Only the twins. Reading the face off "takes a `__this`
        // parameter" was measured and is too wide: the other passes
        // that mint a receiver-first shell do not all register it
        // where this one looks, so five programs died on
        // `redeclaration of function __fwdrecv_<t>`. Which sites get
        // which face moves per route — see `ast/forwarder_synth.rs`.
        let face = if target.starts_with("__smany_") {
            crate::ast::ShellFace::ReceiverFirst
        } else {
            crate::ast::ShellFace::Plain
        };
        let forward_name = if face == crate::ast::ShellFace::ReceiverFirst {
            format!("__fwdrecv_{target}")
        } else {
            format!("__forward_{target}")
        };
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
            face,
        ));
        renames.insert(target.clone(), forward_name);
    }
    (new_decls, renames)
}
