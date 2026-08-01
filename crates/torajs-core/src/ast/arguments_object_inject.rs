//! T-31 argc injection pair — split from arguments_object.rs when
//! the unmapped-arguments knife pushed it past the 500-line limit
//! (rotation 271). Verbatim moves: `inject_argc_params` (synthetic
//! `__torajs_real_argc` / `__torajs_argv` param injection) and
//! `prepend_static_argc` (static call-site argc prepend).

use super::{Ast, Expr, Param, Stmt};

/// T-31 — inject `__torajs_real_argc: number` at param[0] for each
/// uses_real_argc fn. Done before the body-rewrite walk so the
/// typechecker (which runs after desugar) sees the new signature
/// and so the recursive `arguments.length` rewrite can resolve
/// `Ident("__torajs_real_argc")` cleanly.
pub(super) fn inject_argc_params(
    ast: &mut Ast,
    uses_real_argc: &std::collections::HashSet<String>,
    iife_real_argc: &std::collections::HashSet<String>,
    value_real_argc: &std::collections::HashSet<String>,
    value_argv_fns: &std::collections::HashSet<String>,
    method_argv_fns: &std::collections::HashSet<String>,
) {
    if uses_real_argc.is_empty()
        && iife_real_argc.is_empty()
        && value_real_argc.is_empty()
        && value_argv_fns.is_empty()
        && method_argv_fns.is_empty()
    {
        return;
    }
    for s in ast.stmts.iter_mut() {
        if let Stmt::FnDecl { name, params, .. } = s {
            // Chunk 613 — IIFE closures put the synthetic param
            // AFTER the hidden `__env` (first USER param slot);
            // RFC 20260708 value-form closures share that slot, and
            // knife-4a method bodies put it after `__this` — the
            // same index, and `build_boxed_entry` finds it at
            // `params[env_count]` in both shapes.
            let at = if iife_real_argc.contains(name)
                || value_real_argc.contains(name)
                || value_argv_fns.contains(name)
                || method_argv_fns.contains(name)
            {
                1
            } else if uses_real_argc.contains(name) {
                0
            } else {
                continue;
            };
            params.insert(
                at,
                Param {
                    name: "__torajs_real_argc".into(),
                    type_ann: Some("number".into()),
                    default: None,
                    is_rest: false,
                },
            );
            // argv-face bodies also take the adapter's raw argv
            // pointer right after the argc slot.
            if value_argv_fns.contains(name) || method_argv_fns.contains(name) {
                params.insert(
                    at + 1,
                    Param {
                        name: "__torajs_argv".into(),
                        type_ann: Some("__argvptr()".into()),
                        default: None,
                        is_rest: false,
                    },
                );
            }
        }
    }
}

/// T-31 + chunk 613 — prepend the argc argument at static call
/// sites: every direct-Ident call to a `uses_real_argc` top-level
/// fn gets `Number(args.len())` as new arg[0], and each qualifying
/// IIFE call site gets its static count (the closure's `__env` is
/// not part of the Call args, so the count lands in the injected
/// first-USER-param slot).
pub(super) fn prepend_static_argc(
    ast: &mut Ast,
    uses_real_argc: &std::collections::HashSet<String>,
    iife_call_sites: Vec<usize>,
) {
    // T-31 — arena walk: every Call whose callee is a direct Ident to
    // a uses_real_argc fn gets `Number(args.len())` prepended as new
    // arg[0]. args.len() at this point is the user-passed count BEFORE
    // T-28's trailing-undef pad runs in check.rs / ssa_lower. The
    // checker sees the prepended arg, accepts the call (the remaining
    // user params are all Any and qualify for T-28 pad), and ssa_lower
    // lowers the prepended Number as ConstI64 matching the callee's
    // `: number` first param.
    if !uses_real_argc.is_empty() {
        let n = ast.exprs.len();
        for i in 0..n {
            let (callee, args_clone) = match &ast.exprs[i] {
                Expr::Call { callee, args } => (*callee, args.clone()),
                _ => continue,
            };
            let name = match ast.get_expr(callee) {
                Expr::Ident(n) => n.clone(),
                _ => continue,
            };
            if !uses_real_argc.contains(&name) {
                continue;
            }
            let argc = args_clone.len();
            let argc_lit = ast.add_expr(Expr::Number(argc as f64));
            let mut new_args = Vec::with_capacity(argc + 1);
            new_args.push(argc_lit);
            new_args.extend(args_clone);
            ast.exprs[i] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }

    // Chunk 613 — prepend the static argc at each qualifying IIFE
    // call site (the closure's `__env` is not part of the Call args,
    // so the count lands in the injected first-USER-param slot).
    for i in iife_call_sites {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let callee = *callee;
        let args_clone = args.clone();
        let argc_lit = ast.add_expr(Expr::Number(args_clone.len() as f64));
        let mut new_args = Vec::with_capacity(args_clone.len() + 1);
        new_args.push(argc_lit);
        new_args.extend(args_clone);
        ast.exprs[i] = Expr::Call {
            callee,
            args: new_args,
        };
    }
}
