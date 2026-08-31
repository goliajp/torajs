//! RFC 20260717-objlit-anylane-recv knife 1 — the anylane objlit
//! collector family, extracted from `objlit_nominal.rs` (file-size:
//! the dead-arena guard + construction-site binds overlay pushed the
//! host over the 500-line hard limit). One logical unit: the root
//! collectors (a)/(f) legs, the nested-literal closure walk, and the
//! binding leg of the detached-method widen (its returned-literal
//! twin sits in `objlit_nominal_returned.rs`).

use std::collections::HashMap;

use super::{Expr, ExprId, Stmt};
// The detached-method widen family lives in its own sibling (file
// size); the re-export keeps `objlit_nominal_returned`'s import
// path unchanged.
pub(super) use super::objlit_nominal_widen::{
    callee_positions, widen_detached_method_objlits, widen_inner,
};

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
///   ToPropertyKey the key;
/// - (h) a `__proto__: v` PropertyName field (rotation 434) — §B.3.1
///   makes it a [[Prototype]] set, which only the dynobj lane can
///   express (`emit_dynobj_proto_field`); the struct lane would
///   record it as an own data field, a silent-wrong. The property
///   SHORTHAND spelling (`{ __proto__ }`) is an ordinary own field
///   (`objlit_shorthand_proto_exprs`) and keeps the nominal stamp.
///
/// - (j) both arguments of `new Proxy(t, h)` — a proxy reaches its
///   target and its handler only through the any lane, and a
///   nominal struct answers differently there (its fields are not
///   configurable, so a forwarded `delete` refuses);
///
/// - (i) an ACCESSOR- or METHOD-bearing literal whose binding a
///   LATER statement pushes onto the dynobj lane
///   (`Object.defineProperty(o, …)` / `Object.setPrototypeOf(o, …)` /
///   `delete o.x` / `o[k] = v`) — the one leg that does not read the
///   literal's own site, and the only one that asks
///   [`crate::dynobj_degrade`] rather than re-deriving a rule
///   ([`super::objlit_nominal_degraded`]).
///
/// - (k) a method-bearing top-level literal the any-promote verdict
///   will box into an Any slot — computed by
///   [`crate::ast_refs_any_promote::promoted_method_objlits`] against
///   the pre-`objlit_nominal` snapshot and merged in by `run` (it
///   needs the whole `&Ast`, which this collector's split-borrow
///   signature cannot take).
///
/// Still NOT covered: closure-valued callees and method-shape calls
/// whose any params the SSA route serves — those keep the nominal
/// stamp and the dynobj-init guard rejects their recv members
/// loudly.
pub(super) fn collect_anylane_objlits(
    stmts: &[Stmt],
    exprs: &[Expr],
    objlit_method_exprs: &std::collections::HashSet<ExprId>,
    shorthand_proto: &std::collections::HashSet<ExprId>,
    computed_keys: &HashMap<ExprId, ExprId>,
    computed_accessors: &HashMap<ExprId, bool>,
) -> std::collections::HashSet<u32> {
    let mut marked: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut roots: Vec<ExprId> = Vec::new();
    collect_any_let_inits(stmts, &mut roots);
    // (i) — a literal with a receiver face whose BINDING a later
    // statement degrades to the dynobj lane; see
    // [`super::objlit_nominal_degraded`].
    roots.extend(super::objlit_nominal_degraded::degraded_recv_face_objlits(
        stmts,
        exprs,
        objlit_method_exprs,
        computed_keys,
        computed_accessors,
    ));
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
                        || (n == "__proto__" && !shorthand_proto.contains(fe))
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
            // (j) — both arguments of `new Proxy(t, h)`
            // (RFC 20260823-proxy-substrate 刀 2). A proxy reaches
            // its target and its handler ONLY through the any lane —
            // trap lookup, forwarded [[Get]], forwarded [[Delete]] —
            // and a nominal struct answers a different set of
            // questions there (a struct field is not configurable,
            // so a forwarded `delete` refuses). Same shape as (e):
            // the position, not the value, decides the lane.
            Expr::New {
                class_name, args, ..
            } if class_name == "Proxy" => {
                for a in args.iter().take(2) {
                    roots.push(*a);
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
