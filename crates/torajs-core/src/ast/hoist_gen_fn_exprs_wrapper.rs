//! The wrapper route of [`super::hoist_gen_fn_exprs`] — a generator
//! expression whose body captures an enclosing local or reads its
//! own self-name. Moved out verbatim (rotation 635) so the hoist's
//! per-expression loop keeps its headroom; the capture policy this
//! implements is documented on the parent module.

use super::{Ast, Expr, ExprId, Param, Stmt};

/// True when a snapshot of `captures` taken at each wrapper call is
/// observably identical to live by-reference capture: none of the
/// captured names is ever reassigned (`Assign` / `PostIncr`) or
/// rebound by a for-of / for-in loop head anywhere in the program.
/// Conservative by name — a same-named binding in an unrelated scope
/// also refuses (compound assignment already parses to `Assign`, and
/// pre-increment to `x = x + 1`, so the two expr forms cover every
/// write spelling).
fn captures_snapshot_safe(ast: &Ast, captures: &[String]) -> bool {
    let is_cap = |eid: ExprId| matches!(&ast.exprs[eid.0 as usize], Expr::Ident(n) if captures.iter().any(|c| c == n));
    let mut arrow_bodies: Vec<&Stmt> = Vec::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } if is_cap(*target) => {
                return false;
            }
            // Loop heads inside arrow bodies rebind through the same
            // stmt shapes — feed them into the walk below (`for (x
            // of xs)` with no declarator assigns the OUTER x).
            Expr::ArrowFn { body, .. } => arrow_bodies.extend(body.iter()),
            _ => {}
        }
    }
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    stack.extend(arrow_bodies);
    while let Some(s) = stack.pop() {
        match s {
            Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
                if captures.iter().any(|c| c == var_name) {
                    return false;
                }
                stack.push(body);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                stack.push(then_branch);
                if let Some(e) = else_branch {
                    stack.push(e);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                stack.push(body)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    stack.push(i);
                }
                stack.push(body);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    stack.extend(c.body.iter());
                }
                if let Some(d) = default {
                    stack.extend(d.iter());
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                stack.extend(body.iter());
                stack.extend(catch_body.iter());
                if let Some(f) = finally_body {
                    stack.extend(f.iter());
                }
            }
            Stmt::Block(b) | Stmt::Multi(b) => stack.extend(b.iter()),
            Stmt::FnDecl { body, .. } => stack.extend(body.iter()),
            Stmt::ClassDecl {
                methods,
                static_methods,
                ..
            } => {
                for m in methods.iter().chain(static_methods.iter()) {
                    stack.extend(m.body.iter());
                }
            }
            Stmt::ExportDecl { inner: Some(i), .. } => {
                stack.push(i);
            }
            _ => {}
        }
    }
    true
}

/// Give one hoisted generator expression the wrapper route when its
/// body has captures or reads its self-name: the expression slot `i`
/// becomes `(orig params) => __genexpr_N(<caps>, orig params)` and
/// `params` gains the captured names as leading params. A body with
/// neither leaves both untouched. `bound` is every name the program
/// binds anywhere — a free name outside it is not a capture.
#[allow(clippy::too_many_arguments)]
pub(super) fn route_wrapper(
    ast: &mut Ast,
    i: usize,
    eid: ExprId,
    name: &str,
    has_recv: bool,
    params: &mut Vec<Param>,
    body: &[Stmt],
    global_names: &[String],
    bound: &std::collections::HashSet<String>,
) {
    // §15.5.5 (RFC 20260810 knife 3) — the self-name binds inside
    // the body, never a capture the enclosing scope must supply. A
    // body that READS it takes the wrapper route below: the
    // wrapper ArrowFn reuses this ExprId, so `fn_expr_self_names`
    // already names it and the knife-1 self-slot binds the name
    // inside the wrapper — which then forwards its own cell as a
    // leading argument the prep pass moves onto `this.<name>`. A
    // body that WRITES it has those writes rewritten below before
    // the prep can turn them into field stores.
    let self_name = ast.fn_expr_self_names.get(&eid).cloned();
    let mut captures =
        crate::ast::free_vars::free_vars_of_arrow(ast, params, body, global_names, &[], None);
    let self_ref = self_name
        .as_ref()
        .is_some_and(|sn| captures.iter().any(|c| c == sn));
    if self_ref {
        let sn = self_name.as_ref().unwrap();
        // §15.5.5 — a write to the self-name is a strict TypeError or
        // a sloppy no-op, and after the prep rewrite it would lower to
        // a silent `this` field store: rewrite the body's write sites
        // first (`hoist_gen_fn_exprs_self_write`). A same-named write
        // in an unrelated scope finds no site here, and the snapshot
        // is then exactly right.
        if !captures_snapshot_safe(ast, std::slice::from_ref(sn)) {
            let strict = !ast.body_is_sloppy(body);
            super::hoist_gen_fn_exprs_self_write::rewrite_self_name_writes(ast, body, sn, strict);
        }
        captures.retain(|c| c != sn);
    }
    // A free name no scope in the program binds is not a capture:
    // it is an implicit global or an undeclared reference, and the
    // hoisted body's own lanes answer those — the same way the
    // declaration form does (`hoist_gen_fn_exprs_names`, rotation
    // 635). Asked after the self-name is settled: that name is
    // bound by the function env, not by any declaration.
    captures.retain(|c| bound.contains(c));
    if !captures.is_empty() || self_ref {
        // Wrapper route (module doc "Capture policy") — refuse
        // the combinations whose plumbing is unproven, and any
        // capture where a call-time snapshot is observably
        // different from live capture.
        if has_recv || params.iter().any(|p| p.is_rest) {
            panic!(
                "generator expression capturing outer binding(s) {captures:?} combined \
                 with a rest param or receiver-threaded `this` is not yet supported \
                 (RFC 20260713-generator-fn-value-substrate residual)"
            );
        }
        // RFC 20260912 — a capture that is written somewhere cannot
        // ride a call-time snapshot: it is read BY REFERENCE through
        // the wrapper's env instead. The wrapper hands its own cell to
        // the factory (a self-name, real or minted) and the body keeps
        // the bare name; `desugar_generators` and the lowering do the
        // rest (`GenEnvInfo`).
        let (byref, snap): (Vec<String>, Vec<String>) = captures
            .iter()
            .cloned()
            .partition(|c| !captures_snapshot_safe(ast, std::slice::from_ref(c)));
        let cell = if self_ref || !byref.is_empty() {
            Some(self_name.clone().unwrap_or_else(|| {
                let minted = format!("__genenv_{}", eid.0);
                ast.fn_expr_self_names.insert(eid, minted.clone());
                minted
            }))
        } else {
            None
        };
        if !byref.is_empty() {
            ast.gen_env_captures.insert(
                name.to_string(),
                super::GenEnvInfo {
                    wrapper: eid,
                    cell_field: cell.clone().expect("a byref capture always has a cell"),
                    names: byref.clone(),
                    class: String::new(),
                },
            );
        }
        // The expression slot becomes `(orig params) =>
        // __genexpr_N(<cell>, <snap caps>, orig params)`; lift_arrow_fns
        // turns it into a closure carrying the real env. The factory
        // gains the cell and the snapshot captures as leading params,
        // which the generator prep moves onto `this.<name>` — the body
        // compiles untouched. A by-reference capture is read in the
        // wrapper body only so the wrapper CAPTURES it (its env holds
        // the box the iterator methods bind).
        let callee = ast.add_expr(Expr::Ident(name.to_string()));
        let mut args: Vec<ExprId> =
            Vec::with_capacity(usize::from(cell.is_some()) + snap.len() + params.len());
        // The wrapper's own cell rides first — inside the wrapper
        // the self-name resolves through the knife-1 self-slot.
        if let Some(c) = cell.as_ref() {
            args.push(ast.add_expr(Expr::Ident(c.clone())));
        }
        for c in &snap {
            args.push(ast.add_expr(Expr::Ident(c.clone())));
        }
        for p in params.iter() {
            args.push(ast.add_expr(Expr::Ident(p.name.clone())));
        }
        let call = ast.add_expr(Expr::Call { callee, args });
        let mut wrapper_body: Vec<Stmt> = byref
            .iter()
            .map(|c| Stmt::Expr(ast.add_expr(Expr::Ident(c.clone()))))
            .collect();
        wrapper_body.push(Stmt::Return(Some(call)));
        ast.exprs[i] = Expr::ArrowFn {
            params: params.clone(),
            return_type: None,
            body: wrapper_body,
        };
        let lead_params: Vec<Param> = cell
            .iter()
            .chain(snap.iter())
            .map(|c| Param {
                name: c.clone(),
                type_ann: Some("any".to_string()),
                default: None,
                is_rest: false,
            })
            .collect();
        params.splice(0..0, lead_params);
    }
}
