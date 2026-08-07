//! Constructed fn-expression bindings join the static-argv face.
//!
//! `var F = function () { this.n = arguments.length; }; new F(1,2,3)`
//! lifts the fn-expr to `__closure_N(__env, __this, …user)` and binds
//! `let F = Closure __closure_N`; the factory (`__fnctor_F`) calls the
//! BINDING name. So the face never admitted the body: `__closure_N`
//! carries `__env` (env_fns → KeepLoud), and its call sites hide
//! behind `Ident(F)`, which the named-fn collector's per-fn-name scan
//! can't see. The loud reject (`unknown identifier arguments`) is the
//! whole S13.2.2 arguments residue.
//!
//! The shape is the IIFE knife's twin: every call site of the binding
//! is statically known. Admit when ALL arena uses of the binding name
//! are either direct Call callees agreeing on ONE argc (no spread) or
//! `.prototype` member reads (the factory's own §10.2.2-step-5 read
//! plus the classic `F.prototype.m = …` pattern — reads the cell,
//! never calls, so it cannot disturb the argc vote). Any other use is
//! a value escape reaching call sites that know nothing about the
//! injected extras — refuse, keep the loud reject.
//!
//! The admitted key is the LIFTED fn name (`__closure_N`): that is
//! the FnDecl the face's param injection and body rewrite walk by.
//! The receiver slot (`__this`, present when the ctor-this promotion
//! fired) is excluded from the argc the same way the method face
//! excludes `__cm_`'s.

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet};

pub(super) fn collect_ctor_bound_static_argv(ast: &Ast) -> HashMap<String, usize> {
    // Top-level closure bindings whose lifted body touches arguments.
    // binding name → (lifted fn name, receiver slots at call sites).
    let mut bound: HashMap<String, (String, usize)> = HashMap::new();
    for s in &ast.stmts {
        let Stmt::LetDecl { name, init, .. } = s else {
            continue;
        };
        let Expr::Closure { fn_name, .. } = ast.get_expr(*init) else {
            continue;
        };
        let Some(decl_params) = ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name: n,
                params,
                body,
                ..
            } if n == fn_name
                && (body_has_arguments_length(ast, body)
                    || body_has_non_length_arguments_touch(ast, body)) =>
            {
                Some(params)
            }
            _ => None,
        }) else {
            continue;
        };
        // `__env` is always params[0] on a lifted closure; the
        // promoted receiver, when present, sits right after it and
        // occupies one leading slot of every call's argument list.
        let this_slots = usize::from(decl_params.get(1).is_some_and(|p| p.name == "__this"));
        bound.insert(name.clone(), (fn_name.clone(), this_slots));
    }
    if bound.is_empty() {
        return HashMap::new();
    }
    // A fn-local binding reusing the name would blend foreign uses
    // into the scan — same conservative guard as the named collector
    // (the t262 harness's own `func` param is exactly this trap).
    let mut fn_local_names: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                fn_local_names.insert(p.name.clone());
            }
            crate::ast_collect_bindings::collect_local_binding_names(body, &mut fn_local_names);
        }
    }
    let mut out: HashMap<String, usize> = HashMap::new();
    for (binding, (fn_name, this_slots)) in bound {
        if fn_local_names.contains(&binding) {
            continue;
        }
        let eids: HashSet<u32> = (0..ast.exprs.len() as u32)
            .filter(|&i| matches!(&ast.exprs[i as usize], Expr::Ident(n) if *n == binding))
            .collect();
        let mut legal: HashSet<u32> = HashSet::new();
        let mut argcs: HashSet<usize> = HashSet::new();
        let mut poisoned = false;
        for e in &ast.exprs {
            match e {
                Expr::Call { callee, args } if eids.contains(&callee.0) => {
                    if args
                        .iter()
                        .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
                    {
                        poisoned = true;
                        break;
                    }
                    legal.insert(callee.0);
                    argcs.insert(args.len().saturating_sub(this_slots));
                }
                Expr::Member { obj, name } if eids.contains(&obj.0) && name == "prototype" => {
                    legal.insert(obj.0);
                }
                _ => {}
            }
        }
        if poisoned || eids.difference(&legal).next().is_some() || argcs.len() != 1 {
            continue;
        }
        out.insert(fn_name, argcs.into_iter().next().unwrap());
    }
    out
}
