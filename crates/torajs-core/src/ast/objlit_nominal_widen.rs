//! RFC 20260813-detached-objlit-method — the WIDEN half of the
//! anylane objlit family, split out of `objlit_nominal_anylane.rs`
//! (file size: the Proxy-argument leg (j) put the host exactly on
//! the 500-line cap).
//!
//! Its sibling COLLECTS the literals that already lower through the
//! dynobj lane; this file changes what a literal's annotation SAYS,
//! so that lane becomes the one it lowers through. The two answer
//! different questions and the split is along that seam.

use std::collections::HashMap;

use super::{Expr, ExprId, Stmt};

/// RFC 20260813-detached-objlit-method — widen `const o = { m() {
/// this… } }` to the (a)-leg spelling when the program reads `o.m` as
/// a VALUE rather than calling it.
///
/// A nominal method's body takes `__this: __ObjLit_n` and reads the
/// receiver at struct offsets, but a detached call has no receiver to
/// hand it: the callee is an ordinary `Type::Closure` binding, so it
/// lowers to the env-first `CallIndirect` — which carries no receiver
/// slot AT ALL, leaving the body's third parameter reading whatever
/// register happens to hold it. Measured: `const t = o.read; t()`
/// SIGSEGVs, and so does `t.call({ n: 9 })`, whose explicit thisArg
/// that same arm silently drops.
///
/// The any lane answers all of it — the receiver-first `__this: any`
/// shape takes §10.2.1.2's undefined receiver for a bare call and an
/// explicit thisArg through argv[0]. Selecting it is NOT a matter of
/// marking the literal here: the marked set must stay ⊆ the literals
/// that actually lower through the dynobj lane, and a bare `const`
/// binding lowers nominal. Marking alone gives the method an `any`
/// receiver while the call site still hands it a struct pointer —
/// measured as silent-wrong direct calls (`o.read()` answered
/// `undefined`, `o.bump()` NaN), which is worse than the crash. So
/// the widen is the whole knife: the annotation becomes what the user
/// could have written, and every existing lane follows from it.
///
/// The binding leg here takes `Expr::Member` reads off a bare
/// `Ident` bound by a `LetDecl` whose init IS the literal — the
/// syntactically certain subset. Callee position is what separates a
/// read from a call, so `o.read()` does not widen while
/// `o.read.call(x)` does (there `o.read` is the callee's OBJECT).
/// Binding names are not scope-resolved: a same-named binding
/// elsewhere can widen this one, which only ever costs the nominal
/// receiver's struct-offset reads — the (f) leg's trade.
///
/// r380 adds the other two receiver shapes. A call RESULT
/// (`makeCounter().read`) has no binding to annotate at all and is
/// forced at the `return` instead — [`super::objlit_nominal_returned`].
/// A receiver reached through a PARAMETER (`function via(p){ return
/// p.read }`) does have one, in the caller: [`propagate_param_reads`]
/// carries the read back to it, and this leg lands the widen there.
pub(crate) fn widen_detached_method_objlits(
    stmts: &mut [Stmt],
    exprs: &mut Vec<Expr>,
    spans: &mut Vec<crate::lexer::Span>,
) {
    {
        let mut reads = value_read_members(exprs);
        if !reads.is_empty() {
            propagate_param_reads(stmts, exprs, &mut reads);
            let admit = |name: &str, init: ExprId| {
                matches!(&exprs[init.0 as usize], Expr::ObjectLit { fields } if fields
                .iter()
                .any(|(f, fe)| {
                    f.as_str().is_some_and(|f| reads.contains(&(name, f)))
                        && matches!(&exprs[fe.0 as usize], Expr::Closure { .. })
                }))
            };
            widen_inner(stmts, &admit);
        }
    }
    let returned = super::objlit_nominal_returned::value_read_call_members(exprs);
    if !returned.is_empty() {
        super::objlit_nominal_returned::force_returned_literals(stmts, exprs, spans, &returned);
    }
}

/// ExprIds sitting in callee position — what separates reading a
/// member as a VALUE from calling it.
pub(crate) fn callee_positions(exprs: &[Expr]) -> std::collections::HashSet<u32> {
    exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, .. } => Some(callee.0),
            _ => None,
        })
        .collect()
}

/// `(receiver binding, member)` pairs the program reads as a value.
fn value_read_members(exprs: &[Expr]) -> std::collections::HashSet<(&str, &str)> {
    let callees = callee_positions(exprs);
    exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Expr::Member { obj, name } if !callees.contains(&(i as u32)) => {
                match &exprs[obj.0 as usize] {
                    Expr::Ident(b) => Some((b.as_str(), name.as_str())),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// r380 — carry a value-read back across a PARAMETER to the caller's
/// argument binding. `function via(p){ return p.read }` reads the
/// member off `p`, but the literal lives on the caller's
/// `const obj = { … }`, so the binding leg alone never sees it and
/// the detached call SIGSEGVs.
///
/// Only the caller's BINDING needs widening — measured on the parent
/// commit by splitting the two spellings apart: annotating the
/// parameter `: any` while leaving the binding alone still SIGSEGVs,
/// while widening the binding alone answers everything with the
/// parameter's structural annotation untouched. So this knife never
/// rewrites a signature; it only grows the read set the binding leg
/// already consults.
///
/// Fixpoint because an argument can itself be another fn's parameter
/// (`a(obj)` → `b(p)` → reads `q.m`). Parameter names are not
/// scope-resolved and a duplicated fn name drops out of the map
/// entirely, the same trade the (f) leg takes.
fn propagate_param_reads<'a>(
    stmts: &[Stmt],
    exprs: &'a [Expr],
    reads: &mut std::collections::HashSet<(&'a str, &'a str)>,
) {
    let params = collect_fn_param_names(stmts);
    if params.is_empty() {
        return;
    }
    loop {
        let mut added = false;
        for e in exprs {
            let Expr::Call { callee, args } = e else {
                continue;
            };
            let Expr::Ident(f) = &exprs[callee.0 as usize] else {
                continue;
            };
            let Some(pnames) = params.get(f) else {
                continue;
            };
            for (i, a) in args.iter().enumerate() {
                let Expr::Ident(arg_name) = &exprs[a.0 as usize] else {
                    continue;
                };
                let Some(pn) = pnames.get(i) else {
                    continue;
                };
                let members: Vec<&'a str> = reads
                    .iter()
                    .filter(|(recv, _)| *recv == pn.as_str())
                    .map(|(_, m)| *m)
                    .collect();
                for m in members {
                    added |= reads.insert((arg_name.as_str(), m));
                }
            }
        }
        if !added {
            return;
        }
    }
}

/// Per-FnDecl parameter names for [`propagate_param_reads`]. Shape
/// mirrors [`collect_fn_any_params`], including its ambiguity drop:
/// the map is name-keyed while the real binding resolves per scope,
/// so a duplicated name could pair the wrong parameter list with a
/// call site.
fn collect_fn_param_names(stmts: &[Stmt]) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_fn_param_names_inner(stmts, &mut map, &mut seen);
    map
}

fn collect_fn_param_names_inner(
    stmts: &[Stmt],
    map: &mut HashMap<String, Vec<String>>,
    seen: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
        {
            if !seen.insert(name.clone()) {
                map.remove(name);
            } else {
                map.insert(
                    name.clone(),
                    params.iter().map(|p| p.name.clone()).collect(),
                );
            }
            collect_fn_param_names_inner(body, map, seen);
        }
    }
}

/// Statement recursion shared by every widen leg — same shape as
/// [`collect_any_let_inits`], which is what will pick the widened
/// declaration up on the (a) leg. `admit` answers, for one
/// unannotated declaration, whether its init literal belongs on the
/// any lane: the detached-method leg below asks about value-read
/// members, [`super::objlit_nominal_degraded`] asks the dynobj-degrade
/// collector.
pub(crate) fn widen_inner(stmts: &mut [Stmt], admit: &dyn Fn(&str, ExprId) -> bool) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                // An explicit annotation is the user's word on the
                // shape; only an unannotated binding is ours to widen.
                if type_ann.is_none() && admit(name.as_str(), *init) {
                    *type_ann = Some("any".to_string());
                }
            }
            Stmt::FnDecl { body, .. } => widen_inner(body, admit),
            Stmt::Block(inner) | Stmt::Multi(inner) => widen_inner(inner, admit),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                widen_inner(std::slice::from_mut(then_branch.as_mut()), admit);
                if let Some(eb) = else_branch {
                    widen_inner(std::slice::from_mut(eb.as_mut()), admit);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                widen_inner(std::slice::from_mut(body.as_mut()), admit);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    widen_inner(std::slice::from_mut(i.as_mut()), admit);
                }
                widen_inner(std::slice::from_mut(body.as_mut()), admit);
            }
            _ => {}
        }
    }
}
