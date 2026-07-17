//! RFC 20260714-objlit-accessor blade 1 — give a method-bearing object
//! literal a nominal identity so its methods can bind `this`.
//!
//! `{ a: 5, m() { return this.a * 2 } }` did not compile: `this` inside
//! a method body is rewritten to `Ident("__this")` by
//! `desugar_classes_pass2` (which walks the WHOLE expr arena, on the
//! assumption — stated in its own comment — that `this` only ever
//! appears inside class methods), and nothing binds `__this` at an
//! object literal. `free_vars_of_arrow` then collects it as a capture
//! and `check_closure` rejects it: "closure `__closure_0` references
//! unknown identifier `__this`".
//!
//! The fix mirrors what classes already do. A class method's `__this`
//! param is annotated with the CLASS NAME (`desugar_classes_emit`), and
//! the class itself is turned into a `Stmt::TypeDecl` whose fields are
//! the DATA fields only (`desugar_classes_pass3`); the checker's
//! TypeDecl pre-pass (`check_pipeline`) resolves that into
//! `aliases[C] = Struct(..)` before any function body is checked. So
//! `__this: C` resolves, and there is no recursion — because methods
//! are NOT layout fields, the struct type never mentions itself.
//!
//! We mint the same shape for an object literal: a synthetic nominal
//! alias `__ObjLit_<n>` plus a `TypeDecl` carrying its data fields. The
//! one thing a class method cannot do and an object-literal method must
//! is CAPTURE (`function f() { let c = 0; return { bump() { return ++c
//! } } }`), so the lifted FnDecl keeps its `__env` and takes `__this` as
//! a second param: `(__env, __this, ...user)`.
//!
//! The field annotation is minted as `__mth(<recv>|<user params>)->R`.
//! The marker rides the prefix (the `__clsargc(` idiom from RFC
//! 20260708): `parse_type` decodes it exactly like `__cls(` so the SSA
//! sig carries the receiver and `CallIndirect`'s argv lines up, while
//! the checker drops the leading receiver so `o.m(x)` types with the
//! arity the user wrote.
//!
//! Runs INSIDE `desugar_implicit_generics`, after `preinfer_closure_sigs`
//! and before its main loop: the lifted closures exist by then (so we
//! can patch their FnDecls) and `fn_sigs` is populated (so we can type
//! the data fields), but the object literal's own `__inlobj(...)` ann
//! has not been minted yet — which is what lets our `__mth(` field ann
//! flow out through the normal return-type inference.

use std::collections::HashMap;

use super::{AstExprsView, Expr, ExprId, Param, Stmt, binds_to_params, infer_expr_ann_with};

/// One method field of one object literal, resolved to the pieces the
/// apply phase needs. Collected read-only so the arena walk and the
/// arena mutation don't overlap.
struct MethodPatch {
    /// The `Expr::Closure` slot (same ExprId the `ArrowFn` had).
    eid: ExprId,
    /// The lifted FnDecl to give a `__this` param.
    fn_name: String,
    /// The object literal's synthetic nominal alias.
    objlit_ty: String,
    /// The field name this method sits under.
    field: String,
}

pub(crate) fn run(
    stmts: &mut Vec<Stmt>,
    exprs: &mut Vec<Expr>,
    objlit_method_exprs: &std::collections::HashSet<ExprId>,
    objlit_method_fields: &mut HashMap<String, Vec<String>>,
    outer_binds: &HashMap<String, String>,
    fn_sigs: &mut HashMap<String, String>,
    fnexpr_recv_fns: &mut std::collections::HashSet<String>,
) {
    if objlit_method_exprs.is_empty() {
        return;
    }
    let anylane = collect_anylane_objlits(stmts, exprs);
    let mut type_decls: Vec<Stmt> = Vec::new();
    let mut patches: Vec<MethodPatch> = Vec::new();
    let mut any_patches: Vec<(ExprId, String)> = Vec::new();

    {
        let view: AstExprsView = &*exprs;
        let bind_params = binds_to_params(outer_binds);
        let mut next = 0usize;
        for i in 0..view.len() {
            let Expr::ObjectLit { fields } = &view[i] else {
                continue;
            };
            if !fields.iter().any(|(_, e)| objlit_method_exprs.contains(e)) {
                continue;
            }
            // Only a method that actually REFERENCES `this` changes
            // shape. `free_vars_of_arrow` already answered that question:
            // pass-2 rewrote the body's `this` to a bare `__this` Ident,
            // so it is sitting in the capture list.
            //
            // This is what keeps the blast radius at zero. A method that
            // never says `this` is ABI-identical to a plain closure
            // field, and plenty of code hands such a closure to consumers
            // that know nothing about receivers — most sharply
            // `Object.defineProperty(o, k, { get() {..}, set(v) {..} })`,
            // whose descriptor IS an object literal with method
            // shorthands, and whose accessor-define path pulls the
            // closure straight out of the field. Giving those a `__this`
            // silently shifted every argument (a setter fixture started
            // storing NaN).
            let uses_this = |feid: &ExprId| {
                matches!(&view[feid.0 as usize],
                    Expr::Closure { captures, .. } if captures.iter().any(|c| c == "__this"))
            };
            // An ACCESSOR always takes the receiver, even when its body
            // never says `this` — test262's target case is exactly that
            // (`{ get v() { count++; return 2 } }` closes over an outer
            // `count`). It can afford to: `__getter_<n>` / `__setter_<n>`
            // are synthetic names, and the accessor lane is the only
            // thing that ever reads those slots. A plain method can't —
            // its closure gets handed to consumers that pass no receiver.
            let needs_recv = |fname: &String, feid: &ExprId| {
                objlit_method_exprs.contains(feid)
                    && (uses_this(feid)
                        || fname.starts_with("__getter_")
                        || fname.starts_with("__setter_"))
            };
            // RFC 20260717-objlit-anylane-recv knife 1 — a literal the
            // dynobj lane will consume gets NO nominal identity: its
            // receiver is a dynobj cell at runtime, so a `__this:
            // __ObjLit_n` body's struct-offset reads are garbage
            // (this-using method through any = SIGSEGV probe). Promote
            // this-USING members to the `__this: any` receiver-first
            // shape instead (the fn-expr face mechanics — body reads
            // dispatch through the any lane, `FLAG_CLOSURE_RECV_FIRST`
            // routes the receiver into argv[0]). A this-free member —
            // accessor included — keeps the plain closure ABI; the
            // dynobj-init accessor install picks generic kinds for it.
            if anylane.contains(&(i as u32)) {
                for (_, feid) in fields {
                    if objlit_method_exprs.contains(feid)
                        && uses_this(feid)
                        && let Expr::Closure { fn_name, .. } = &view[feid.0 as usize]
                    {
                        any_patches.push((*feid, fn_name.clone()));
                    }
                }
                continue;
            }
            if !fields.iter().any(|(n, e)| needs_recv(n, e)) {
                continue;
            }
            let objlit_ty = format!("__ObjLit_{next}");
            next += 1;

            // METHODS ARE LAYOUT FIELDS — they are own enumerable
            // properties, and `this.other()` has to resolve against the
            // receiver's own type. That is safe only because a `__mth(`
            // slot's signature does NOT name the receiver: were it in
            // there, `__ObjLit_n`'s layout would refer to itself, and
            // `parse_struct` has no memo-before-fill to break the cycle.
            //
            // A data field the ann sniffer can't type falls back to
            // `any` rather than dropping out of the layout, so
            // `this.<field>` still resolves.
            let mut td_fields: Vec<(String, String)> = Vec::new();
            let mut method_names: Vec<String> = Vec::new();
            for (fname, feid) in fields {
                if needs_recv(fname, feid) {
                    let Expr::Closure { fn_name, .. } = &view[feid.0 as usize] else {
                        // Not lifted (`lift_arrow_fns` runs first, so
                        // this shouldn't happen). Leave it be rather
                        // than guess a shape.
                        continue;
                    };
                    method_names.push(fname.clone());
                    td_fields.push((fname.clone(), "__mth_placeholder".to_string()));
                    patches.push(MethodPatch {
                        eid: *feid,
                        fn_name: fn_name.clone(),
                        objlit_ty: objlit_ty.clone(),
                        field: fname.clone(),
                    });
                    continue;
                }
                // Data field, or a `this`-free method — the latter keeps
                // the plain closure-slot ABI, so its ann comes from the
                // same sniffer as any other fn-valued field.
                let ann = infer_expr_ann_with(view, *feid, &bind_params, outer_binds, fn_sigs)
                    .unwrap_or_else(|| "any".to_string());
                td_fields.push((fname.clone(), super::retag_field_fn_ann(&ann)));
            }
            objlit_method_fields.insert(objlit_ty.clone(), method_names);
            type_decls.push(Stmt::TypeDecl {
                name: objlit_ty,
                type_params: Vec::new(),
                fields: td_fields,
            });
        }
    }

    if !any_patches.is_empty() {
        super::fnexpr_this::promote_recv_any(stmts, exprs, &any_patches, fnexpr_recv_fns);
    }
    if patches.is_empty() {
        return;
    }
    apply_patches(stmts, exprs, &patches, &mut type_decls, fn_sigs);
    stmts.extend(type_decls);
}

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
///   out of the map, both directions keeping (f) ⊆ the SSA route).
///
/// Still NOT covered: closure-valued callees and method-shape calls
/// whose any params the SSA route serves — those keep the nominal
/// stamp and the dynobj-init guard rejects their recv members
/// loudly.
fn collect_anylane_objlits(stmts: &[Stmt], exprs: &[Expr]) -> std::collections::HashSet<u32> {
    let mut marked: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut roots: Vec<ExprId> = Vec::new();
    collect_any_let_inits(stmts, &mut roots);
    let fn_any_params = collect_fn_any_params(stmts);
    for (i, e) in exprs.iter().enumerate() {
        match e {
            // (b) — literal `undefined` field value.
            Expr::ObjectLit { fields }
                if fields.iter().any(
                    |(_, fe)| matches!(&exprs[fe.0 as usize], Expr::Ident(n) if n == "undefined"),
                ) =>
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

/// Phase 2 — for each collected `MethodPatch`, drop `__this` from the
/// closure's capture list (it's a receiver, not a capture; it only
/// landed there because pass-2 rewrote `this` to a bare Ident before
/// anyone knew where it was bound), then rewrite the matching FnDecl's
/// param list + re-publish its sig as `__mth(...)->ret` (receiver-less
/// per the module doc), and fill the `__mth_placeholder` the collect
/// phase parked in the TypeDecl.
fn apply_patches(
    stmts: &mut [Stmt],
    exprs: &mut [Expr],
    patches: &[MethodPatch],
    type_decls: &mut [Stmt],
    fn_sigs: &mut HashMap<String, String>,
) {
    // `__this` is a receiver, not a capture. It only landed in the
    // capture list because pass-2 rewrote `this` to a bare Ident before
    // anyone knew where it was bound.
    for p in patches {
        if let Expr::Closure { captures, .. } = &mut exprs[p.eid.0 as usize] {
            captures.retain(|c| c != "__this");
        }
    }

    for p in patches {
        let Some(caps) = closure_captures(exprs, p.eid) else {
            continue;
        };
        let mut mth_ann: Option<String> = None;
        for s in stmts.iter_mut() {
            let Stmt::FnDecl {
                name,
                params,
                return_type,
                ..
            } = s
            else {
                continue;
            };
            if *name != p.fn_name {
                continue;
            }
            // The `__env(...)` ann lists the captured names; `__this` is
            // out of the capture list now, so it has to leave the ann
            // too — otherwise the lowerer reads an env slot nothing
            // stored.
            if let Some(env) = params.first_mut()
                && env.name == "__env"
            {
                env.type_ann = Some(format!("__env({})", caps.join("|")));
            }
            if !params.iter().any(|q| q.name == "__this") {
                let at = usize::from(params.first().is_some_and(|q| q.name == "__env"));
                params.insert(
                    at,
                    Param {
                        name: "__this".to_string(),
                        // The NOMINAL alias — same shape as a class
                        // method's `__this: C`. Resolving it gives the
                        // body typed field reads AND sibling-method
                        // calls (`this.other()`).
                        type_ann: Some(p.objlit_ty.clone()),
                        default: None,
                        is_rest: false,
                    },
                );
            }
            // Re-publish the sig: `preinfer_closure_sigs` ran before
            // `__this` existed and spells every closure `__fn(`. The
            // `__mth(` params are the USER params only — the receiver
            // must stay out of every Type (see the module doc).
            let ret = return_type.clone().unwrap_or_else(|| "void".to_string());
            let param_anns: Vec<String> = params
                .iter()
                .filter(|q| q.name != "__env" && q.name != "__this")
                .map(|q| q.type_ann.clone().unwrap_or_else(|| "any".to_string()))
                .collect();
            let ann = format!("__mth({})->{}", param_anns.join("|"), ret);
            fn_sigs.insert(p.fn_name.clone(), ann.clone());
            mth_ann = Some(ann);
            break;
        }
        // Fill the placeholder the collect phase parked in the TypeDecl:
        // the ann needs the FnDecl's params, which only exist post-patch.
        if let Some(ann) = mth_ann {
            for td in type_decls.iter_mut() {
                let Stmt::TypeDecl { name, fields, .. } = td else {
                    continue;
                };
                if *name != p.objlit_ty {
                    continue;
                }
                for (fname, fty) in fields.iter_mut() {
                    if *fname == p.field {
                        *fty = ann.clone();
                    }
                }
                break;
            }
        }
    }
}

fn closure_captures(exprs: &[Expr], eid: ExprId) -> Option<Vec<String>> {
    match &exprs[eid.0 as usize] {
        Expr::Closure { captures, .. } => Some(captures.clone()),
        _ => None,
    }
}
