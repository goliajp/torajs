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
//! The marker rides the prefix (the `__cls(`-marker idiom from RFC
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
pub(super) struct MethodPatch {
    /// The `Expr::Closure` slot (same ExprId the `ArrowFn` had).
    pub(super) eid: ExprId,
    /// The lifted FnDecl to give a `__this` param.
    pub(super) fn_name: String,
    /// The object literal's synthetic nominal alias.
    pub(super) objlit_ty: String,
    /// The field name this method sits under.
    pub(super) field: String,
}

pub(crate) fn run(
    stmts: &mut Vec<Stmt>,
    exprs: &mut Vec<Expr>,
    sloppy: bool,
    spans: &mut Vec<crate::lexer::Span>,
    objlit_method_exprs: &std::collections::HashSet<ExprId>,
    objlit_shorthand_proto_exprs: &std::collections::HashSet<ExprId>,
    objlit_method_fields: &mut HashMap<String, Vec<String>>,
    outer_binds: &HashMap<String, String>,
    objlit_site_binds: &HashMap<u32, HashMap<String, String>>,
    fn_sigs: &mut HashMap<String, String>,
    fnexpr_recv_fns: &mut std::collections::HashSet<String>,
    objlit_computed_keys: &HashMap<ExprId, ExprId>,
    objlit_computed_accessors: &HashMap<ExprId, bool>,
) {
    if objlit_method_exprs.is_empty() {
        return;
    }
    // RFC 20260813-detached-objlit-method — widen FIRST: the (a) leg
    // of the collector below is what picks the widened binding up.
    super::objlit_nominal_anylane::widen_detached_method_objlits(stmts, exprs, spans);
    // Rotation 461 — the same (a)-leg spelling for a literal whose
    // binding a LATER statement degrades to the dynobj lane.
    super::objlit_nominal_degraded::widen_degraded_accessor_objlits(
        stmts,
        exprs,
        objlit_computed_keys,
        objlit_computed_accessors,
    );
    let anylane = super::objlit_nominal_anylane::collect_anylane_objlits(
        stmts,
        exprs,
        objlit_shorthand_proto_exprs,
    );
    let mut type_decls: Vec<Stmt> = Vec::new();
    let mut patches: Vec<MethodPatch> = Vec::new();
    let mut any_patches: Vec<(ExprId, String)> = Vec::new();
    let mut recvless_accessors: Vec<String> = Vec::new();

    {
        let view: AstExprsView = &*exprs;
        let mut next = 0usize;
        for i in 0..view.len() {
            let Expr::ObjectLit { fields } = &view[i] else {
                continue;
            };
            if !fields.iter().any(|(_, e)| objlit_method_exprs.contains(e)) {
                continue;
            }
            // Dead-arena guard — rewrite passes (the arguments-object
            // rewrite among them) re-add composite exprs and leave the
            // original node in the arena, sharing the method Closure
            // ExprIds. This flat scan then minted a SECOND TypeDecl
            // from the stale copy, and its patch (first writer wins)
            // pinned the method's `__this` to the stale layout. The
            // construction-site walk in `closure_capture_anns` only
            // reaches live nodes, so its snapshot keys are the live
            // set; a stale copy misses and is skipped.
            if !objlit_site_binds.contains_key(&(i as u32)) {
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

            let site_binds = site_binds_for(outer_binds, objlit_site_binds, i as u32);
            let site_params = binds_to_params(&site_binds);

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
                    // Rotation 461 — the this-FREE accessors, whose
                    // receiver slot is declared and never read (see
                    // `settle_collected`).
                    if !uses_this(feid) {
                        recvless_accessors.push(fn_name.clone());
                    }
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
                let ann = infer_expr_ann_with(view, *feid, &site_params, &site_binds, fn_sigs)
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

    super::objlit_nominal_settle::settle_collected(super::objlit_nominal_settle::Settle {
        stmts,
        exprs,
        any_patches,
        patches,
        recvless_accessors,
        type_decls,
        fn_sigs,
        fnexpr_recv_fns,
        sloppy,
        spans,
    });
}

/// Field-value idents resolve against the literal's OWN
/// construction-site binds (the `closure_capture_anns` snapshot)
/// first; the program-wide by-name map is only the fallback. Without
/// the overlay, `{ px: x }` under an `x: number` param took a LATER
/// fn's `x: any` ann and laid the slot out as any while the fill
/// wrote raw bits.
fn site_binds_for(
    outer_binds: &HashMap<String, String>,
    objlit_site_binds: &HashMap<u32, HashMap<String, String>>,
    site: u32,
) -> HashMap<String, String> {
    let mut m = outer_binds.clone();
    if let Some(s) = objlit_site_binds.get(&site) {
        m.extend(s.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    m
}

/// Phase 2 — for each collected `MethodPatch`, drop `__this` from the
/// closure's capture list (it's a receiver, not a capture; it only
/// landed there because pass-2 rewrote `this` to a bare Ident before
/// anyone knew where it was bound), then rewrite the matching FnDecl's
/// param list + re-publish its sig as `__mth(...)->ret` (receiver-less
/// per the module doc), and fill the `__mth_placeholder` the collect
/// phase parked in the TypeDecl.
pub(super) fn apply_patches(
    stmts: &mut [Stmt],
    exprs: &mut [Expr],
    patches: &[MethodPatch],
    type_decls: &mut [Stmt],
    fn_sigs: &mut HashMap<String, String>,
    fnexpr_recv_fns: &mut std::collections::HashSet<String>,
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
                body,
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
            // …and re-decide the RETURN for a method that returns the
            // receiver. The same "ran before `__this` existed" problem
            // hits the return sniff, but worse: `__this` was not merely
            // unresolvable, it resolved through a PROGRAM-WIDE by-name
            // annotation map, where every class method in the program —
            // including the injected `Error` subclasses — contributes a
            // `__this` entry and the last one wins. So `{ v: 5, self()
            // { return this; } }` typed `self` as returning
            // `ReferenceError`, and every use of it failed with a
            // mismatch against a class the program never mentions. This
            // is the same collision the field-value sniff above already
            // guards with its construction-site overlay.
            //
            // Only the bare `return this` shape is re-decided, and only
            // when every return in the body is that: a recorded return
            // type is indistinguishable from one the USER wrote, so a
            // broader re-sniff would overwrite explicit annotations.
            if returns_only_this(exprs, body) {
                *return_type = Some(p.objlit_ty.clone());
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
            // The receiver is this body's first declared param, which
            // is exactly what `fnexpr_recv_fns` means — so the closure
            // cell carries `FLAG_CLOSURE_RECV_FIRST` and a call that
            // arrives through the runtime dispatcher (`String(o)` and
            // the other coercions reach the method via
            // OrdinaryToPrimitive, not through the static call site)
            // puts the receiver in argv[0] instead of leaving `this`
            // undefined. The any-lane sibling already registers here;
            // the four static gates that also read this set all key on
            // the closure expression BEING the callback or accessor
            // face, which a literal's own field never is.
            fnexpr_recv_fns.insert(p.fn_name.clone());
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

/// Does every `return` in `body` hand back the bare receiver, and is
/// there at least one? `this` has been rewritten to a bare `__this`
/// ident by the time this pass runs.
///
/// A body that returns the receiver on one path and something else on
/// another answers false: the return type is then a union this pass has
/// no way to spell, and leaving the sniffed annotation alone keeps the
/// mismatch visible instead of replacing it with a wrong-but-quiet one.
fn returns_only_this(exprs: &[Expr], body: &[Stmt]) -> bool {
    let mut saw = false;
    fn walk(exprs: &[Expr], s: &Stmt, saw: &mut bool) -> bool {
        match s {
            Stmt::Return(Some(eid)) => {
                *saw = true;
                matches!(&exprs[eid.0 as usize], Expr::Ident(n) if n == "__this")
            }
            Stmt::Return(None) => {
                *saw = true;
                false
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().all(|s| walk(exprs, s, saw)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk(exprs, then_branch, saw)
                    && else_branch.as_ref().is_none_or(|eb| walk(exprs, eb, saw))
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                walk(exprs, body, saw)
            }
            Stmt::For { body, .. } => walk(exprs, body, saw),
            // A nested fn's returns are its own, not this method's.
            Stmt::FnDecl { .. } => true,
            _ => true,
        }
    }
    body.iter().all(|s| walk(exprs, s, &mut saw)) && saw
}

fn closure_captures(exprs: &[Expr], eid: ExprId) -> Option<Vec<String>> {
    match &exprs[eid.0 as usize] {
        Expr::Closure { captures, .. } => Some(captures.clone()),
        _ => None,
    }
}
