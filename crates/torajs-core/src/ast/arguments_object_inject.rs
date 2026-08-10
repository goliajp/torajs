//! T-31 argc injection pair — split from arguments_object.rs when
//! the unmapped-arguments knife pushed it past the 500-line limit
//! (rotation 271). Verbatim moves: `inject_argc_params` (synthetic
//! `__torajs_real_argc` / `__torajs_argv` param injection) and
//! `prepend_static_argc` (static call-site argc prepend).

use super::{Ast, Expr, Param, Stmt};

/// T-31 — inject the synthetic argc/argv params ahead of the
/// body-rewrite walk so the typechecker (which runs after desugar)
/// sees the new signature and the recursive rewrites can resolve the
/// synthetic idents cleanly.
///
/// RFC 20260810-indirect-argc-abi S3.4 — env-first faces are off the
/// `__torajs_real_argc` injection entirely: every reader on those
/// faces (length reads S3.2, materialize take-count S3.3) rides the
/// S1 hidden-ABI `__torajs_argc`, so the injected slot would only be
/// a dead param the A-station has to double-feed. What remains:
///
/// - `uses_real_argc` (head-less top-level fns): argc at param[0] —
///   no hidden slot exists there until the S1-extension blade.
/// - `value_argv_fns` (env-first argv face): ONLY the raw argv
///   pointer, at the first user slot after `__env`.
/// - `method_argv_fns` (`__cm_` this-first): argc + argv after
///   `__this` — that entry family has no hidden slot either.
pub(super) fn inject_argc_params(
    ast: &mut Ast,
    uses_real_argc: &std::collections::HashSet<String>,
    value_argv_fns: &std::collections::HashSet<String>,
    method_argv_fns: &std::collections::HashSet<String>,
) {
    if uses_real_argc.is_empty() && value_argv_fns.is_empty() && method_argv_fns.is_empty() {
        return;
    }
    let real_argc = || Param {
        name: "__torajs_real_argc".into(),
        type_ann: Some("number".into()),
        default: None,
        is_rest: false,
    };
    let argv = || Param {
        name: "__torajs_argv".into(),
        type_ann: Some("__argvptr()".into()),
        default: None,
        is_rest: false,
    };
    for s in ast.stmts.iter_mut() {
        if let Stmt::FnDecl { name, params, .. } = s {
            if uses_real_argc.contains(name) {
                params.insert(0, real_argc());
            } else if value_argv_fns.contains(name) {
                params.insert(1, argv());
            } else if method_argv_fns.contains(name) {
                params.insert(1, real_argc());
                params.insert(2, argv());
            }
        }
    }
}

/// T-31 — prepend the argc argument at static call sites: every
/// direct-Ident call to a `uses_real_argc` top-level fn gets
/// `Number(args.len())` as new arg[0]. (The IIFE tier's call-site
/// prepend retired in S3.2 along with its param injection — that
/// face reads the S1 hidden-ABI argc.)
pub(super) fn prepend_static_argc(
    ast: &mut Ast,
    uses_real_argc: &std::collections::HashSet<String>,
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
}
