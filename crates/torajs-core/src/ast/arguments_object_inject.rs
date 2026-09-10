//! T-31 argv injection — split from arguments_object.rs when the
//! unmapped-arguments knife pushed it past the 500-line limit
//! (rotation 271). The `injected-argc` half (param injection +
//! `prepend_static_argc` call-site prepend) retired with the RFC
//! 20260810-indirect-argc-abi H2 blade: every argc tier now reads
//! the S1 hidden sig slot.

use super::{Ast, Param, Stmt};

/// T-31 — inject the synthetic argv param ahead of the body-rewrite
/// walk so the typechecker (which runs after desugar) sees the new
/// signature and the recursive rewrites can resolve the synthetic
/// ident cleanly.
///
/// RFC 20260810-indirect-argc-abi — the argc channel is the S1
/// hidden sig slot on every tier (env-first S3.2/S3.4, `__cm_`
/// this-first S1-T1/T2, head-less S1-H1/H2), so only the raw argv
/// pointer still rides an injected param:
///
/// - `value_argv_fns` (env-first argv face): at the first user slot
///   after `__env`.
/// - `method_argv_fns` (`__cm_` this-first): right after `__this`.
/// - `this_first_argv_fns` (this-first direct-call lane, rotation 616 / 627):
///   after `__this` AND `__new_target`, both of which a ctor
///   declaration always carries.
/// - `headless_argv_fns` (RFC 20260816-headless-argv-face): at
///   position 0 — a head-less body has no head param, so "right
///   after the head" degenerates to the front, exactly as the H1
///   hidden argc slot does on the sig side.
pub(super) fn inject_argc_params(
    ast: &mut Ast,
    value_argv_fns: &std::collections::HashSet<String>,
    method_argv_fns: &std::collections::HashSet<String>,
    headless_argv_fns: &std::collections::HashSet<String>,
    this_first_argv_fns: &std::collections::HashMap<String, usize>,
) {
    if value_argv_fns.is_empty()
        && method_argv_fns.is_empty()
        && headless_argv_fns.is_empty()
        && this_first_argv_fns.is_empty()
    {
        return;
    }
    let argv = || Param {
        name: "__torajs_argv".into(),
        type_ann: Some("__argvptr()".into()),
        default: None,
        is_rest: false,
    };
    for s in ast.stmts.iter_mut() {
        if let Stmt::FnDecl { name, params, .. } = s {
            if let Some(&head) = this_first_argv_fns.get(name) {
                // "Right after the head", and the head is as wide as
                // the declaration says: one slot for a twin or a
                // promoted declaration, two for a ctor (`__this`,
                // `__new_target`).
                params.insert(head, argv());
            } else if value_argv_fns.contains(name) || method_argv_fns.contains(name) {
                params.insert(1, argv());
            } else if headless_argv_fns.contains(name) {
                params.insert(0, argv());
            }
        }
    }
}
