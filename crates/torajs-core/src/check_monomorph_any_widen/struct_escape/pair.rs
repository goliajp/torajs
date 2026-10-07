//! The twin pair behind one relay shell: the target's all-`any` twin
//! and the shell twin forwarding to it.

use std::collections::{HashMap, VecDeque};

use super::closure::{all_any, inner_suffix};
use super::{construction_site, mint_shell_twin, twin_name};
use crate::ast::{Ast, ExprId, Param, Stmt};
use crate::check::Checker;
use crate::check_monomorph::WorkItem;
use crate::check_monomorph_any_widen::{RetFace, emit_widened_spec};
use crate::ssa_lower_generics_monomorph::Generics;

/// Mint `<target>$$anyws` (a generator's desugar-time twin stands in)
/// and the shell twin over it, for a `shell` whose value may be handed
/// what its target's typed slots cannot hold. True when either was added.
#[allow(clippy::too_many_arguments)]
pub(super) fn mint_pair(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    shell: &str,
    target: &str,
    params: &[Param],
) -> bool {
    let Some(site) = construction_site(owned_ast, shell) else {
        return false;
    };
    let emitted = mono_decls.len();
    // a generator's twin was cut from its source before the machine
    // was built: its factory body is no body to clone
    let generator_twin = crate::ast::generator_any_twin(owned_ast, target);
    let target_twin = generator_twin.clone().unwrap_or_else(|| twin_name(target));
    if generator_twin.is_none() && !c.globals.contains_key(&target_twin) {
        mirror_target_tables(owned_ast, target, &target_twin);
        let idxs = all_any(params);
        emit_widened_spec(
            c,
            owned_ast,
            generics,
            cache,
            worklist,
            call_retargets,
            mono_decls,
            target,
            &target_twin,
            &inner_suffix(target),
            &idxs,
            RetFace::Any,
        );
    }
    mint_shell_twin(c, owned_ast, shell, params, &target_twin, site);
    mono_decls.len() > emitted || c.globals.contains_key(&twin_name(shell))
}

/// The name-keyed tables the target twin answers to as the target does.
/// The shell twin's forwarding call is a packed direct call of the
/// target twin: the checker folds its rest sentinel back to the one
/// array slot only for names in `rest_arg_prefix`
/// (`check_type_of_call_rest_param::fold_packed_direct_call`). A
/// length-only body reads its hidden argc, and so does its twin, which
/// that forwarding call fills with the shell's own runtime count.
fn mirror_target_tables(ast: &mut Ast, target: &str, twin: &str) {
    if let Some(&prefix) = ast.rest_arg_prefix.get(target) {
        ast.rest_arg_prefix.insert(twin.to_string(), prefix);
    }
    if ast.headless_argc_fns.contains(target) {
        ast.headless_argc_fns.insert(twin.to_string());
    }
}
