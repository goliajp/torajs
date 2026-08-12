//! RFC 20260717-objlit-anylane-recv knife 1 — the anylane objlit
//! collector family, extracted from `objlit_nominal.rs` (file-size:
//! the dead-arena guard + construction-site binds overlay pushed the
//! host over the 500-line hard limit). One logical unit: the root
//! collectors (a)/(f) legs, the nested-literal closure walk, and the
//! binding leg of the detached-method widen (its returned-literal
//! twin sits in `objlit_nominal_returned.rs`).

use std::collections::HashMap;

use super::{Expr, ExprId, Stmt};

/// RFC 20260717-objlit-anylane-recv knife 1 — the syntactically
/// decidable set of object literals that will lower through the
/// dynobj lane, mirroring the SSA-side routes:
///
/// - (a) `let x: any = { ... }` init at any nesting depth — the P3.2
///   `lower_dynobj_init` shortcut (`ssa_lower_stmt_let_decl`);
/// - (b) a literal `undefined` field — the checker's
///   `struct_has_undef_field` widen forces the binding to Any;
/// - (c) `{ ... } as any` — knife 2 widened the `lower_as_cast`
///   promote from empty-literal-only to every ObjectLit, so the
///   cast IS the dynobj route now (synthesized `As`-any wrappers —
///   default-any async returns, generator yields — ride the same
///   leg, and their dynobj face is the sound one: every consumer
///   dispatches through the any lane);
/// - (d) an ObjectLit nested in a marked literal —
///   `lower_dynobj_init` recurses nested literals;
/// - (e) the receiver argument of `Object.defineProperty` /
///   `defineProperties` — the `lower_define_receiver` promote;
/// - (f) a direct-call arg into an explicitly `any`-annotated
///   FnDecl param — `ssa_lower_call_terminal`'s any-param route
///   (`expected == Type::Any && ObjectLit → lower_dynobj_init`).
///   Only the syntactically certain subset: a bare `Ident` callee
///   naming a non-generic FnDecl whose param says `: any` verbatim
///   (an untyped param turns into an implicit generic — its mono
///   instance is NOT Any — and a shadowed/duplicated fn name drops
///   out of the map, both directions keeping (f) ⊆ the SSA route);
/// - (g) a computed-key field (`__computed_N__` sentinel) — the
///   checker types the whole literal Any at every position (RFC
///   20260809 knife 1b), the dynobj lane is the only one that can
///   ToPropertyKey the key.
///
/// Still NOT covered: closure-valued callees and method-shape calls
/// whose any params the SSA route serves — those keep the nominal
/// stamp and the dynobj-init guard rejects their recv members
/// loudly.
pub(super) fn collect_anylane_objlits(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<u32> {
    let mut marked: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut roots: Vec<ExprId> = Vec::new();
    collect_any_let_inits(stmts, &mut roots);
    let fn_any_params = collect_fn_any_params(stmts);
    for (i, e) in exprs.iter().enumerate() {
        match e {
            // (b) — literal `undefined` field value; (g) — a
            // computed-key field (the parser's `__computed_N__`
            // sentinel name): the checker answers Any for the whole
            // literal at EVERY position (RFC 20260809 knife 1b, the
            // any-spread exit), so only the dynobj lane can name the
            // property — a this-using method here must take the
            // `__this: any` receiver-first shape or its receiver
            // writes land on a struct-unbox COPY (the measured
            // `this.x = 99` no-transmit / identity-loss family).
            Expr::ObjectLit { fields }
                if fields.iter().any(|(n, fe)| {
                    n.starts_with("__computed_")
                        || matches!(&exprs[fe.0 as usize], Expr::Ident(x) if x == "undefined")
                }) =>
            {
                roots.push(ExprId(i as u32));
            }
            // (c) — `as any` cast (user-written or synthesized); the
            // strip loop below unwraps the `As` chain to the literal.
            Expr::As { expr, ty_ann } if ty_ann == "any" => {
                roots.push(*expr);
            }
            // (e) — define-family receiver argument.
            Expr::Call { callee, args } => {
                if let Expr::Member { obj, name } = &exprs[callee.0 as usize]
                    && (name == "defineProperty" || name == "defineProperties")
                    && matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Object")
                    && let Some(recv) = args.first()
                {
                    roots.push(*recv);
                }
                // (f) — arg into an explicitly-any FnDecl param.
                if let Expr::Ident(f) = &exprs[callee.0 as usize]
                    && let Some(mask) = fn_any_params.get(f)
                {
                    for (i, a) in args.iter().enumerate() {
                        if mask.get(i).copied().unwrap_or(false) {
                            roots.push(*a);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // (d) — nested literals of a marked literal join the set (strip
    // `as` chains the way `lower_dynobj_init` does).
    while let Some(eid) = roots.pop() {
        let mut cur = eid;
        while let Expr::As { expr, .. } = &exprs[cur.0 as usize] {
            cur = *expr;
        }
        let Expr::ObjectLit { fields } = &exprs[cur.0 as usize] else {
            continue;
        };
        if !marked.insert(cur.0) {
            continue;
        }
        for (_, fe) in fields {
            roots.push(*fe);
        }
    }
    marked
}

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
/// r380 adds the second receiver shape — a call RESULT
/// (`makeCounter().read`), which has no binding to annotate — in
/// [`super::objlit_nominal_returned`]. What remains uncovered is a
/// receiver reached through a PARAMETER (`function via(p){ return
/// p.read }`), where the literal's binding lives in the caller;
/// recorded residue, not a silent narrowing.
pub(super) fn widen_detached_method_objlits(
    stmts: &mut [Stmt],
    exprs: &mut Vec<Expr>,
    spans: &mut Vec<crate::lexer::Span>,
) {
    {
        let reads = value_read_members(exprs);
        if !reads.is_empty() {
            widen_inner(stmts, exprs, &reads);
        }
    }
    let returned = super::objlit_nominal_returned::value_read_call_members(exprs);
    if !returned.is_empty() {
        super::objlit_nominal_returned::force_returned_literals(stmts, exprs, spans, &returned);
    }
}

/// ExprIds sitting in callee position — what separates reading a
/// member as a VALUE from calling it.
pub(super) fn callee_positions(exprs: &[Expr]) -> std::collections::HashSet<u32> {
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

/// Statement recursion of [`widen_detached_method_objlits`] — same
/// shape as [`collect_any_let_inits`], which is what will pick the
/// widened declaration up on the (a) leg.
fn widen_inner(
    stmts: &mut [Stmt],
    exprs: &[Expr],
    reads: &std::collections::HashSet<(&str, &str)>,
) {
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
                if type_ann.is_none()
                    && let Expr::ObjectLit { fields } = &exprs[init.0 as usize]
                    && fields.iter().any(|(f, fe)| {
                        reads.contains(&(name.as_str(), f.as_str()))
                            && matches!(&exprs[fe.0 as usize], Expr::Closure { .. })
                    })
                {
                    *type_ann = Some("any".to_string());
                }
            }
            Stmt::FnDecl { body, .. } => widen_inner(body, exprs, reads),
            Stmt::Block(inner) | Stmt::Multi(inner) => widen_inner(inner, exprs, reads),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                widen_inner(std::slice::from_mut(then_branch.as_mut()), exprs, reads);
                if let Some(eb) = else_branch {
                    widen_inner(std::slice::from_mut(eb.as_mut()), exprs, reads);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                widen_inner(std::slice::from_mut(body.as_mut()), exprs, reads);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    widen_inner(std::slice::from_mut(i.as_mut()), exprs, reads);
                }
                widen_inner(std::slice::from_mut(body.as_mut()), exprs, reads);
            }
            _ => {}
        }
    }
}

/// (f) leg of [`collect_anylane_objlits`] — per-FnDecl mask of the
/// params annotated `: any` verbatim. Generic fns are out (a mono
/// instance's param is not Any) and so are synthesized closure
/// shapes (`__closure_*` / `__forward_*` are never Ident-called). A
/// duplicated fn name drops out entirely: the mask is name-keyed
/// while the SSA route resolves per scope, so an ambiguous name
/// could pair the wrong mask with a call site.
fn collect_fn_any_params(stmts: &[Stmt]) -> HashMap<String, Vec<bool>> {
    let mut map: HashMap<String, Vec<bool>> = HashMap::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_fn_any_params_inner(stmts, &mut map, &mut seen);
    map
}

fn collect_fn_any_params_inner(
    stmts: &[Stmt],
    map: &mut HashMap<String, Vec<bool>>,
    seen: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        if let Stmt::FnDecl {
            name,
            params,
            type_params,
            body,
            ..
        } = s
        {
            if type_params.is_empty()
                && !crate::ast_desugar_implicit_generics::is_synth_closure_name(name)
            {
                if !seen.insert(name.clone()) {
                    // Second decl under this name — ambiguous, drop it.
                    map.remove(name);
                } else {
                    let mask: Vec<bool> = params
                        .iter()
                        .map(|p| p.type_ann.as_deref() == Some("any"))
                        .collect();
                    if mask.iter().any(|b| *b) {
                        map.insert(name.clone(), mask);
                    }
                }
            }
            collect_fn_any_params_inner(body, map, seen);
        }
    }
}

/// (a) leg of [`collect_anylane_objlits`] — `let x: any = <init>`
/// at any statement nesting depth (same recursion shape as
/// `infer_closure_params::collect_let_anns`).
fn collect_any_let_inits(stmts: &[Stmt], out: &mut Vec<ExprId>) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                type_ann: Some(t),
                init,
                ..
            } if t == "any" => out.push(*init),
            Stmt::FnDecl { body, .. } => collect_any_let_inits(body, out),
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_any_let_inits(inner, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_any_let_inits(std::slice::from_ref(then_branch.as_ref()), out);
                if let Some(eb) = else_branch {
                    collect_any_let_inits(std::slice::from_ref(eb.as_ref()), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_any_let_inits(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::Labeled { body, .. } => {
                collect_any_let_inits(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_any_let_inits(std::slice::from_ref(i.as_ref()), out);
                }
                collect_any_let_inits(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}
