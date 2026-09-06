//! Forwarder-decl synthesis — the "what does one `__forward_<t>`
//! look like" half of [`super::forwarders_object`], split to a
//! sibling when the 402-01 generic-target erasure pushed that file
//! past the 500-line limit. The parent keeps the store-site
//! collection / rewrite walk; this file builds the decls it asked
//! for.

use super::{Ast, Param, Stmt};

/// 402-01 — substitute a generic target's own TypeVars with `any`
/// across the forwarder's param / return annotations (the target's
/// `type_params` are read off its FnDecl in place, so every caller
/// of the synthesizer gets the erasure without threading a table).
/// Non-generic targets pass through untouched.
fn erase_generic_typevars(
    ast: &Ast,
    target: &str,
    params: Vec<Param>,
    return_type: Option<String>,
) -> (Vec<Param>, Option<String>) {
    let type_params: Vec<String> = ast
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::FnDecl {
                name, type_params, ..
            } if name == target => Some(type_params.clone()),
            _ => None,
        })
        .unwrap_or_default();
    if type_params.is_empty() {
        return (params, return_type);
    }
    let subst: Vec<(String, String)> = type_params
        .into_iter()
        .map(|tp| (tp, "any".to_string()))
        .collect();
    let params = params
        .into_iter()
        .map(|mut p| {
            if let Some(a) = &mut p.type_ann {
                *a = crate::ssa_lower_generics_monomorph::substitute_in_ann(a, &subst);
            }
            p
        })
        .collect();
    let return_type =
        return_type.map(|rt| crate::ssa_lower_generics_monomorph::substitute_in_ann(&rt, &subst));
    (params, return_type)
}

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
        // 402-01 — a GENERIC target's signature still spells its own
        // TypeVars (`gid<T>(v: T): T`), but the forwarder is a plain
        // FnDecl with no type params to bind them — the checker
        // rejects it loud ("unknown type `T`"). Erase every TypeVar
        // to `any`: the forwarder body's `target(v)` call is then an
        // ordinary generic call site the checker infers at Any, and
        // the mono pass emits the `$$_any` specialization the call
        // retargets to.
        let (params, return_type) = erase_generic_typevars(ast, target, params, return_type);
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
        ));
        renames.insert(target.clone(), forward_name);
    }
    (new_decls, renames)
}
