//! Knife-2 variable-routed face promotion — carved out of
//! `fnexpr_this.rs` when knife 7 (prototype-store faces) pushed it
//! past the 500-line hard limit. Verbatim move; the position walk
//! stays in the parent, which hands its Ident candidates here.

use super::fnexpr_this_faces::FacePatch;
use super::fnexpr_this_names::{
    collect_hoisted_fnexpr_assigns, collect_this_fnexpr_decl_names, name_shadowed_elsewhere,
    peel_as,
};
use super::fnexpr_this_pairing::pair_decls_scoped;
use super::fnexpr_this_shapes::UseShapes;
use super::{Expr, ExprId, Stmt};

/// Knife 2 — variable-routed faces: a face-position Ident promotes
/// only when EVERY use of the binding program-wide is a face read
/// (a single face — the original knife-2 profile — or several
/// faces sharing one closure, the knife-2W multi-face widening;
/// any OTHER read — a direct call, a reassignment target, an alias
/// init — would see the shifted-args closure ABI, the exact
/// silent-wrong the zero-alias bar forbids, so those keep today's
/// loud reject) and the decl's init (const or var — rotation 261) is
/// a marked fn-expr whose body
/// says `this`. The decl lookup recurses through fn bodies (a face
/// inside a function scope resolves its local const — the
/// nested-scope profile), but only a name DECLARED EXACTLY ONCE
/// program-wide promotes: with a same-name decl in another scope a
/// face read cannot be paired to its binding syntactically, and a
/// mispair would stamp RECV on a face whose runtime value is the
/// other binding. Over-removal keeps those loud. Every face ExprId
/// lands in `fnexpr_recv_faces` for the compile-time
/// literal-descriptor lowering; runtime paths read the closure
/// header flag instead.
///
/// Face candidates dedup by ExprId: the position walk can hit the
/// SAME face node more than once — a pre-pass clones a
/// face-position Call (fresh Call + descriptor nodes, leaf arg
/// ExprIds shared), so `{ get: g }`'s single Ident lands in
/// `ident_cands` once per clone. The use-vs-face parity below
/// compares against the arena's UNIQUE Ident nodes, so the face
/// list must be unique too (the pre-2W per-entry `uses == 1` check
/// tolerated duplicates implicitly).

/// The BY-NAME candidate seeding — every profile that enters the
/// promote loop with an empty face list, injected before the parity
/// walk. Returns `(no_static_seed, hoisted_inits)`.
///
/// * Rotation 328 — the ZERO-FACE all-direct-call profile:
///   `var f = function () { …this… }; f();` never stands in a face
///   position, but every guard in the loop already covers it — the
///   use-vs-face parity degenerates to "every use is a direct-call
///   callee", and knife-2W cut 2's `closure_local` lane seeds a boxed
///   `undefined` into the promoted `__this` slot at each such call
///   (§10.2.1.2 strict call-site `this`, the `this_param.rs` blade-1
///   framing — matching bun's module-goal answer). Injecting the
///   binding with an empty face list is the whole knife; any use
///   shape besides a direct call still rejects in the parity check.
/// * 398-06 — the annotated names promote alongside the plain ones
///   but land in `no_static_seed`: their calls take the runtime recv
///   gate instead of the static seed (doc on the census).
/// * Rotation 437 — the hoisted-var spelling of the same profile: a
///   nested-block `var f = function () { …this… }` reaches this pass
///   as a fn-scope `let f: any = Uninit` prelude plus one in-place
///   `f = fn-expr` assignment (doc on the census). The name joins the
///   candidate set here; the init resolves through the assignment in
///   the loop, and only on the ZERO-FACE profile — a face read of a
///   hoisted binding keeps today's loud reject (`fnexpr_recv_locals`
///   registration is what the HOF face consumers key on, and a
///   hoisted name deliberately stays off that list — its any-typed
///   calls ride the runtime recv gate, the 398-06 lane).
#[allow(clippy::type_complexity)]
fn seed_censused_names(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    faces_by_name: &mut std::collections::HashMap<String, Vec<ExprId>>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashMap<String, (ExprId, ExprId)>,
) {
    let mut call_only: Vec<String> = Vec::new();
    let mut annotated_only: Vec<String> = Vec::new();
    collect_this_fnexpr_decl_names(
        stmts,
        exprs,
        fn_expr_exprs,
        &mut call_only,
        &mut annotated_only,
    );
    for name in call_only {
        faces_by_name.entry(name).or_default();
    }
    let no_static_seed: std::collections::HashSet<String> =
        annotated_only.iter().cloned().collect();
    for name in annotated_only {
        faces_by_name.entry(name).or_default();
    }
    let hoisted_inits = collect_hoisted_fnexpr_assigns(exprs, fn_expr_exprs);
    for name in hoisted_inits.keys() {
        faces_by_name.entry(name.clone()).or_default();
    }
    (no_static_seed, hoisted_inits)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn promote_variable_routed(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    closure_argc_locals: &std::collections::HashSet<String>,
    closure_argv_locals: &std::collections::HashSet<String>,
    ident_cands: Vec<(String, ExprId)>,
    call_faces: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
    fnexpr_recv_faces: &mut std::collections::HashSet<ExprId>,
    fnexpr_recv_locals: &mut std::collections::HashSet<String>,
) {
    let mut faces_by_name: std::collections::HashMap<String, Vec<ExprId>> =
        std::collections::HashMap::new();
    for (name, face_eid) in ident_cands {
        let v = faces_by_name.entry(name).or_default();
        if !v.contains(&face_eid) {
            v.push(face_eid);
        }
    }
    let (no_static_seed, hoisted_inits) =
        seed_censused_names(stmts, exprs, fn_expr_exprs, &mut faces_by_name);
    // Every receiver-safe use POSITION in the program, by kind, with
    // the two proof families documented on the module. A binding
    // promotes only if all of its non-face uses land in one of them.
    let shapes = UseShapes::collect(stmts, exprs);
    for (name, face_eids) in &faces_by_name {
        let use_eids: Vec<ExprId> = exprs
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Expr::Ident(n) if n == name))
            .map(|(i, _)| ExprId(i as u32))
            .collect();
        // The hoisted init-assignment's target is the ONE write the
        // profile admits — the census guarantees it is the only
        // Assign-target of this name program-wide, so exempting it
        // leaves every other write to fail the parity as before.
        let hoist_init = hoisted_inits.get(name.as_str());
        let mixed_calls: Vec<ExprId> = use_eids
            .iter()
            .filter(|e| !face_eids.contains(e))
            .filter(|e| hoist_init.is_none_or(|(t, _)| **e != *t))
            .copied()
            .collect();
        if !mixed_calls.iter().all(|e| shapes.admits(*e, name)) {
            continue;
        }
        let mut decls: Vec<(bool, ExprId)> = Vec::new();
        let mut twin_inits: Vec<ExprId> = Vec::new();
        collect_decls_split(stmts, name, false, &mut decls, &mut twin_inits);
        if decls.len() != 1 {
            // 399-03 — a multiply-declared name is no longer a blanket
            // refusal: the scope-paired census proves each binding
            // separately and promotes all or none.
            try_promote_scope_paired(
                stmts,
                exprs,
                fn_expr_exprs,
                closure_argc_locals,
                closure_argv_locals,
                name,
                face_eids,
                &shapes,
                call_faces,
                decls.len() + twin_inits.len(),
                patches,
                fnexpr_recv_faces,
                fnexpr_recv_locals,
            );
            continue;
        }
        // Rotation 261 — a MUTABLE decl (`var f = function () {…}`,
        // the dominant test262 spelling) promotes too: a reassignment
        // is an Assign-target / PostIncr-target Ident, which the
        // use-vs-face parity above already rejects (not a face, not a
        // call callee), so a promoted binding provably never rebinds.
        // What mutability does NOT cover is a same-name shadow from a
        // non-LetDecl declarator (fn param / catch param / loop var)
        // — the by-name Ident walk would pair the shadow's uses with
        // this binding — so those keep the loud reject for var and
        // const alike (the decls-count guard only sees LetDecls).
        let (_mutable, decl_init) = decls[0];
        let mut init = peel_as(exprs, decl_init);
        // Hoisted-var profile: the prelude decl's init is Uninit and
        // the fn-expr hangs off the one admitted assignment. Only the
        // zero-face shape resolves through it (doc at the census
        // injection above).
        let mut from_hoist = false;
        if matches!(exprs[init.0 as usize], Expr::Uninit)
            && face_eids.is_empty()
            && let Some((_, v)) = hoist_init
        {
            init = *v;
            from_hoist = true;
        }
        if fn_expr_exprs.contains(&init)
            && let Expr::Closure { fn_name, captures } = &exprs[init.0 as usize]
            && captures.iter().any(|c| c == "__this")
        {
            if name_shadowed_elsewhere(stmts, name) {
                continue;
            }
            // A binding with a DIRECT CALL must not also ride a
            // boxed-argv call lane: the real-argc prepend contends
            // for the same leading argv slot, and the full-arguments
            // adapter materializes params straight off argv — a
            // `__this` param would eat argv[0]. All stay loud. A
            // `.call`/`.apply` face rides the same `closure_local`
            // replay, so its binding is under the same bar even with
            // zero direct calls. A member access is NOT under
            // the bar (species key 2): it enters no call lane at
            // all, and the store-face + argv combination is exactly
            // the escape-store profile whose adapter order the
            // rotation-338 knife fixed.
            //
            // A REST param used to sit under this bar too, and no
            // longer does (rotation 416): a recv-first body's
            // `__this` IS argv[0] by construction — §10.4.4 says the
            // receiver is not an argument — so the adapter drops the
            // count by one and unboxes the fixed prefix, `__this`
            // included, before the rest window starts. The bar
            // predates that recv slot (RFC 20260808 knife 2). Its
            // cost was the dominant test262 callback spelling,
            // `function (...args) { …this… }`, failing to compile.
            if (mixed_calls.iter().any(|e| shapes.callee.contains(e))
                || face_eids.iter().any(|e| call_faces.contains(e)))
                && (closure_argc_locals.contains(name) || closure_argv_locals.contains(name))
            {
                continue;
            }
            patches.push(FacePatch {
                eid: init,
                fn_name: fn_name.clone(),
            });
            // The twin's own copy of the same declaration, promoted
            // alongside it — see `collect_decls_split` for why the
            // guard above counts one thing and this patches another.
            for t in &twin_inits {
                let t = peel_as(exprs, *t);
                if fn_expr_exprs.contains(&t)
                    && let Expr::Closure { fn_name, captures } = &exprs[t.0 as usize]
                    && captures.iter().any(|c| c == "__this")
                {
                    patches.push(FacePatch {
                        eid: t,
                        fn_name: fn_name.clone(),
                    });
                }
            }
            fnexpr_recv_faces.extend(face_eids.iter().copied());
            // EVERY promoted binding registers, not only the mixed
            // profile: the HOF lowerings detect a variable-routed
            // promoted callback through this set (a pure-face
            // binding left it empty and the loop called the RECV
            // ABI without the `__this` argv slot — `this` read the
            // element box, rotation 260). The direct-call consumer
            // (`ssa_lower_call_closure_local`) still only fires on
            // an actual bare-name call, which a pure-face profile
            // has none of. 398-06 — annotated bindings stay off the
            // static list and take the runtime recv gate instead
            // (doc on the census). Rotation 437 — hoisted-var
            // bindings stay off it too: their prelude decl is
            // any-typed, so every call already rides the runtime
            // recv gate, and the zero-face gate above means no HOF
            // consumer ever needs the name.
            if !no_static_seed.contains(name) && !from_hoist {
                fnexpr_recv_locals.insert(name.clone());
            }
        }
    }
}

/// 399-03 — the multiply-declared path: run knife 2's full
/// per-binding proof on every scope-paired declaration group, and
/// promote ALL of them or none.
///
/// All-or-nothing is load-bearing, not caution: the downstream
/// consumers (`fnexpr_recv_locals`, the direct-call `undefined`
/// seeding in `ssa_lower_call_closure_local`) are name-keyed, so a
/// partially promoted name would shift argv on call sites whose
/// runtime value is an unpromoted closure — the exact silent wrong
/// the zero-alias bar forbids. Each group's proof is the single-decl
/// path's, verbatim in spirit: init is a marked fn-expr still
/// carrying `__this`, every non-face use admits a receiver-safe
/// shape, and a group with a call face or a direct call stays off
/// the boxed-argv lanes. The group count must match the by-name
/// census (`collect_decls_split` mono + twin) — a mismatch means the
/// pairing walk missed a declaration the flat walk can see, and the
/// name keeps the loud reject.
#[allow(clippy::too_many_arguments)]
fn try_promote_scope_paired(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    closure_argc_locals: &std::collections::HashSet<String>,
    closure_argv_locals: &std::collections::HashSet<String>,
    name: &str,
    face_eids: &[ExprId],
    shapes: &UseShapes,
    call_faces: &std::collections::HashSet<ExprId>,
    expected_decls: usize,
    patches: &mut Vec<FacePatch>,
    fnexpr_recv_faces: &mut std::collections::HashSet<ExprId>,
    fnexpr_recv_locals: &mut std::collections::HashSet<String>,
) {
    // A non-LetDecl shadow (fn param / catch / loop var) still voids
    // the name outright — the pairing walk assumes those are absent.
    if name_shadowed_elsewhere(stmts, name) {
        return;
    }
    let Some(groups) = pair_decls_scoped(stmts, exprs, name) else {
        return;
    };
    if groups.len() != expected_decls {
        return;
    }
    let mut plans: Vec<(ExprId, String, Vec<ExprId>)> = Vec::new();
    for g in &groups {
        let init = peel_as(exprs, g.init);
        if !fn_expr_exprs.contains(&init) {
            return;
        }
        let Expr::Closure { fn_name, captures } = &exprs[init.0 as usize] else {
            return;
        };
        if !captures.iter().any(|c| c == "__this") {
            return;
        }
        let g_faces: Vec<ExprId> = g
            .uses
            .iter()
            .filter(|e| face_eids.contains(e))
            .copied()
            .collect();
        if !g
            .uses
            .iter()
            .filter(|e| !g_faces.contains(e))
            .all(|e| shapes.admits(*e, name))
        {
            return;
        }
        // The boxed-argv bar, per group — same reasoning as the
        // single-decl path's.
        if (g.uses.iter().any(|e| shapes.callee.contains(e))
            || g_faces.iter().any(|e| call_faces.contains(e)))
            && (closure_argc_locals.contains(name) || closure_argv_locals.contains(name))
        {
            return;
        }
        plans.push((init, fn_name.clone(), g_faces));
    }
    for (init, fn_name, g_faces) in plans {
        patches.push(FacePatch { eid: init, fn_name });
        fnexpr_recv_faces.extend(g_faces);
    }
    fnexpr_recv_locals.insert(name.to_string());
}

/// Every `LetDecl` of `name`, split by whether it sits inside a
/// receiver-polymorphic TWIN body.
///
/// `desugar_classes_generic_twin` clones each `this`-using method body
/// into `__cmany_<C>__<m>` (and each static one into `__smany_`). So a
/// `const` a program writes ONCE inside a method is DECLARED TWICE in
/// the statement list this census walks, and the `decls.len() != 1`
/// guard above read the copy as a same-name binding in another scope.
/// That is the whole reason `const g = function () { …this… }; g()`
/// answered the enclosing receiver when written at method scope and
/// `undefined` when written at top level — the same program, and the
/// second answer is the §10.2.1.2 one.
///
/// The two halves are deliberately not interchangeable: the guard
/// counts SOURCES (mono bodies only), while the promotion patches the
/// COPIES too. Promoting only the source would be the silent wrong the
/// zero-alias bar exists to prevent — the direct-call seeding is
/// name-keyed, so the twin's `g()` would push an `undefined` into an
/// argv slot its unpromoted closure never declared.
fn collect_decls_split(
    stmts: &[Stmt],
    name: &str,
    in_twin: bool,
    mono: &mut Vec<(bool, ExprId)>,
    twin: &mut Vec<ExprId>,
) {
    for s in stmts {
        if let Stmt::LetDecl {
            mutable,
            name: dn,
            init,
            ..
        } = s
            && dn == name
        {
            if in_twin {
                twin.push(*init);
            } else {
                mono.push((*mutable, *init));
            }
        }
        let nested_twin = in_twin
            || matches!(s, Stmt::FnDecl { name: f, .. }
                    if super::fnexpr_this_names::is_twin_body_name(f));
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_decls_split(inner, name, nested_twin, mono, twin)
        });
    }
}
