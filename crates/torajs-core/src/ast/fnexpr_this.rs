//! RFC 20260717-fnexpr-this-channel knife 1 — give a function
//! expression sitting in an INLINE accessor-face position its
//! call-site `this`.
//!
//! `o.__defineSetter__("y", function (v) { this._y = v })` — the
//! parser encodes the fn-expr as an `Expr::ArrowFn` (marked in
//! `ast.fn_expr_exprs`), `desugar_classes` pass 2 rewrites the body's
//! `this` to a bare `__this` Ident, and `lift_arrow_fns` lists it as a
//! capture — which the checker rejects (`__this` is unbound at the
//! definition scope). ES §10.2.1.2 binds a function expression's
//! `this` at the CALL SITE; for an accessor face that is the property
//! read/write receiver.
//!
//! This pass is the fn-expr mirror of `objlit_nominal::apply_patches`
//! (RFC 20260714-objlit-accessor blade 1): drop `__this` from the
//! closure's capture list (it is a receiver, not a capture), insert it
//! as the first declared param after `__env` (typed `any` — the
//! receiver arrives as a NaN-box through the boxed dual entry), and
//! record the lifted fn name in `ast.fnexpr_recv_fns` so the closure
//! construction site stamps `FLAG_CLOSURE_RECV_FIRST` and the
//! accessor-face lowering marks the AccessorPair kinds byte
//! `ACC_KIND_RECV`.
//!
//! ONLY inline positions promote — the fn-expr must itself be the
//! face argument/field, so the closure value has zero aliases and no
//! receiver-unaware call path can reach it (a promoted body's native
//! signature has an extra leading param; a plain `f(x)` call would
//! shift every argument — the exact silent-wrong B-4 narrow-surface
//! forbids). Covered positions:
//!
//! * `recv.__defineGetter__(k, <fn-expr>)` / `__defineSetter__` —
//!   annex B §B.2.2.2 legacy define (argument 1);
//! * `Object.defineProperty(o, k, { get: <fn-expr>, set: <fn-expr> })`
//!   — the literal descriptor's face fields.
//!
//! Knives 2-5 widened the surface under the same zero-alias bar:
//! variable-routed faces (knife 2 — single-use, nested decls, the 2W
//! multi-face widening, and the 2W cut-2 mixed profile where the
//! only other use shape is a bare-name direct call — seeded
//! `undefined` via `ast.fnexpr_recv_locals`), HOF callback `thisArg`
//! (knife 4), and `defineProperties` / `Object.create` nesting
//! (knife 5). Everything else (alias inits, argument positions,
//! async fn-expr faces) keeps today's loud reject.
//!
//! Runs inside `desugar_implicit_generics` right after
//! `objlit_nominal::run` — `lift_arrow_fns` has produced the
//! `Expr::Closure` nodes and `preinfer_closure_sigs` has already
//! published the user-facing `__fn(P)->R` anns (which deliberately
//! stay `__this`-free, like `__mth(`'s receiver-less spelling).

use super::fnexpr_this_faces::{FacePatch, collect_face, collect_ident_face, literal_desc_faces};
use super::fnexpr_this_recvs::{
    collect_any_binding_names, collect_arraylit_binding_names, collect_mapset_binding_names,
    fn_has_rest_param,
};
use super::{Expr, ExprId, Param, Stmt};

pub(crate) fn run(
    stmts: &mut [Stmt],
    exprs: &mut [Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    objlit_method_exprs: &std::collections::HashSet<ExprId>,
    closure_argc_locals: &std::collections::HashSet<String>,
    closure_argv_locals: &std::collections::HashSet<String>,
    fnexpr_recv_fns: &mut std::collections::HashSet<String>,
    fnexpr_recv_faces: &mut std::collections::HashSet<ExprId>,
    fnexpr_recv_locals: &mut std::collections::HashSet<String>,
) {
    // A face need not be spelled as a fn-expr. `Object.defineProperty(o,
    // k, { get() { return this._y } })` is a METHOD SHORTHAND, which
    // `objlit_nominal` has already given a `__this` typed with the
    // descriptor literal's own nominal alias — and the walk below is
    // what re-anns that to the property receiver. Keying the early-out
    // on fn-exprs alone meant a program whose only face was a shorthand
    // skipped the whole pass, and its getter body typed `this` as the
    // descriptor: `no member ._y on type Struct([("get", ...)])`.
    if fn_expr_exprs.is_empty() && objlit_method_exprs.is_empty() {
        return;
    }
    let mut patches: Vec<FacePatch> = Vec::new();
    let mut ident_cands: Vec<(String, ExprId)> = Vec::new();
    collect_position_faces(stmts, exprs, fn_expr_exprs, &mut patches, &mut ident_cands);
    promote_variable_routed(
        stmts,
        exprs,
        fn_expr_exprs,
        closure_argc_locals,
        closure_argv_locals,
        ident_cands,
        &mut patches,
        fnexpr_recv_faces,
        fnexpr_recv_locals,
    );
    if patches.is_empty() {
        return;
    }
    let pairs: Vec<(ExprId, String)> = patches.into_iter().map(|p| (p.eid, p.fn_name)).collect();
    promote_recv_any(stmts, exprs, &pairs, fnexpr_recv_fns);
}

/// Position walk — collect literal fn-expr faces (and Ident face
/// candidates for knife 2) from every face POSITION in the arena:
/// member-store on an any receiver, `__defineGetter__` /
/// `__defineSetter__`, `Object.defineProperty` inline descriptors,
/// knife-5 `defineProperties` / `Object.create` nested descriptors,
/// and knife-4 HOF callback slots.
fn collect_position_faces(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    let any_recvs = collect_any_binding_names(stmts);
    let arraylit_recvs = collect_arraylit_binding_names(stmts, exprs);
    let mapset_recvs = collect_mapset_binding_names(stmts, exprs);
    for i in 0..exprs.len() {
        // Member-store face — `anyRecv.m = <fn-expr>` (expando method
        // on a wrapper / dynobj receiver). The stored closure is only
        // reachable back through the receiver's props, so every call
        // rides the runtime any-method dispatch, which reads the
        // FLAG_CLOSURE_n header bit and seeds the receiver —
        // zero-alias by construction for a literal fn-expr RHS. An
        // Ident RHS (`o.m = f`) is a face POSITION for knife-2's
        // use-shape analysis: the binding promotes only when its
        // remaining uses are all face reads / direct calls, same bar
        // as every other variable-routed face. (The Index-target
        // twin `o["k"] = fn` already rides this arm through the
        // parser's Member desugar.)
        if let Expr::Assign { target, value } = &exprs[i] {
            if let Expr::Member { obj, .. } = &exprs[target.0 as usize]
                && matches!(&exprs[obj.0 as usize], Expr::Ident(n) if any_recvs.contains(n))
            {
                collect_face(stmts, exprs, *value, fn_expr_exprs, patches);
                collect_ident_face(exprs, *value, ident_cands);
            }
            continue;
        }
        let Expr::Call { callee, args } = &exprs[i] else {
            continue;
        };
        match &exprs[callee.0 as usize] {
            // `recv.__defineGetter__(k, face)` / `__defineSetter__`.
            Expr::Member { name, .. }
                if name == "__defineGetter__" || name == "__defineSetter__" =>
            {
                if let Some(face) = args.get(1) {
                    collect_face(stmts, exprs, *face, fn_expr_exprs, patches);
                    collect_ident_face(exprs, *face, ident_cands);
                }
            }
            // `Object.defineProperty(o, k, { get: face, set: face })` —
            // the descriptor must be an INLINE object literal; a
            // variable-routed descriptor aliases its faces (knife 2).
            Expr::Member { obj, name } if name == "defineProperty" => {
                if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Object") {
                    continue;
                }
                let Some(desc) = args.get(2) else { continue };
                for face in literal_desc_faces(exprs, *desc) {
                    collect_face(stmts, exprs, face, fn_expr_exprs, patches);
                    collect_ident_face(exprs, face, ident_cands);
                }
            }
            // Knife 5 — `Object.defineProperties(o, { k: { get: face } })`
            // / `Object.create(proto, { k: { get: face } })`: the nested
            // descriptor-of-descriptors form. Both layers must be INLINE
            // object literals so every face keeps zero aliases (same
            // narrow-surface bar as the flat descriptor above).
            Expr::Member { obj, name } if name == "defineProperties" || name == "create" => {
                if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Object") {
                    continue;
                }
                let Some(props) = args.get(1) else { continue };
                let Expr::ObjectLit { fields } = &exprs[props.0 as usize] else {
                    continue;
                };
                let descs: Vec<ExprId> = fields.iter().map(|(_, deid)| *deid).collect();
                for desc in descs {
                    for face in literal_desc_faces(exprs, desc) {
                        collect_face(stmts, exprs, face, fn_expr_exprs, patches);
                        collect_ident_face(exprs, face, ident_cands);
                    }
                }
            }
            // Rotation 260 — `Array.from(iterable, <fn-expr>,
            // thisArg?)`: the static mapFn slot. Only the INLINE
            // fn-expr promotes (zero aliases by construction); the
            // from lowering's map loop threads the boxed thisArg
            // ahead of (elem, i) exactly like the trio kernels.
            Expr::Member { obj, name } if name == "from" => {
                if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Array") {
                    continue;
                }
                if let Some(cb) = args.get(1) {
                    collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
                    collect_ident_face(exprs, *cb, ident_cands);
                }
            }
            // Knife 4 — array HOF callback position over a
            // syntactically-certain `any` receiver
            // (`recv.forEach(<fn-expr>, thisArg?)`): the any-lane HOF
            // kernels read the closure flag and seed argv[0] with the
            // thisArg (or the buffer's undefined padding), so the
            // promoted body's `__this` is exactly §23.1.3's T.
            //
            // A syntactically-certain TYPED array receiver (const
            // ArrayLit init) promotes for the inlined-loop trio only
            // (`forEach` / `map` / `filter` — the shapes
            // `ssa_lower_call_arr_ho` owns and threads a thisArg
            // through); other receivers keep today's loud reject.
            Expr::Member { obj, name } if is_hof_method(name) => {
                let typed_ok = matches!(name.as_str(), "forEach" | "map" | "filter");
                // Typed Map / Set receivers ride the same channel for
                // forEach only (their sole callback-bearing method —
                // the mapset kernels thread the thisArg box).
                let mapset_ok = name == "forEach";
                // An INLINE array-literal receiver
                // (`[1,2].forEach(<fn-expr>, thisArg)`) is the
                // arraylit_recvs shape without the binding hop — same
                // inlined-loop trio, same thisArg thread.
                let inline_arraylit = typed_ok && matches!(&exprs[obj.0 as usize], Expr::Array(_));
                if !inline_arraylit
                    && !matches!(&exprs[obj.0 as usize], Expr::Ident(n)
                    if any_recvs.contains(n)
                        || (typed_ok && arraylit_recvs.contains(n))
                        || (mapset_ok && mapset_recvs.contains(n)))
                {
                    continue;
                }
                if let Some(cb) = args.first() {
                    collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
                    // A variable-routed callback rides knife 2's
                    // zero-alias profile. The HOF lowerings detect
                    // the promoted binding through
                    // `fnexpr_recv_locals` (the Closure-shape
                    // detection alone ran the loop with shifted
                    // args — probed rotation 260, fixed same
                    // rotation).
                    collect_ident_face(exprs, *cb, ident_cands);
                }
            }
            _ => {}
        }
    }
}

/// Knife 2 — variable-routed faces: a face-position Ident promotes
/// only when EVERY use of the binding program-wide is a face read
/// (a single face — the original knife-2 profile — or several
/// faces sharing one closure, the knife-2W multi-face widening;
/// any OTHER read — a direct call, a reassignment target, an alias
/// init — would see the shifted-args closure ABI, the exact
/// silent-wrong the zero-alias bar forbids, so those keep today's
/// loud reject) and the const init is a marked fn-expr whose body
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
#[allow(clippy::too_many_arguments)]
fn promote_variable_routed(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    closure_argc_locals: &std::collections::HashSet<String>,
    closure_argv_locals: &std::collections::HashSet<String>,
    ident_cands: Vec<(String, ExprId)>,
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
    // Knife 2W cut 2 — every Ident node standing in direct-call
    // callee position (`h(args)`). A mixed binding's non-face uses
    // must ALL be members of this set: a direct call of a promoted
    // closure seeds `undefined` into the `__this` argv slot (the
    // closure-local call arm, driven by `fnexpr_recv_locals`), so it
    // is the second — and last — use shape with a receiver-correct
    // call path. Any other use (an alias init, an argument position,
    // a container store, a comparison) has no such path and keeps
    // the whole binding unpromoted (loud).
    let callee_idents: std::collections::HashSet<ExprId> = exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, .. } if matches!(&exprs[callee.0 as usize], Expr::Ident(_)) => {
                Some(*callee)
            }
            _ => None,
        })
        .collect();
    for (name, face_eids) in &faces_by_name {
        let use_eids: Vec<ExprId> = exprs
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Expr::Ident(n) if n == name))
            .map(|(i, _)| ExprId(i as u32))
            .collect();
        let mixed_calls: Vec<ExprId> = use_eids
            .iter()
            .filter(|e| !face_eids.contains(e))
            .copied()
            .collect();
        if !mixed_calls.iter().all(|e| callee_idents.contains(e)) {
            continue;
        }
        let mut decls: Vec<(bool, ExprId)> = Vec::new();
        collect_decls_by_name(stmts, name, &mut decls);
        if decls.len() != 1 {
            continue;
        }
        let (mutable, init) = decls[0];
        if !mutable
            && fn_expr_exprs.contains(&init)
            && let Expr::Closure { fn_name, captures } = &exprs[init.0 as usize]
            && captures.iter().any(|c| c == "__this")
        {
            // A mixed binding must not also ride a boxed-argv call
            // lane: the real-argc prepend contends for the same
            // leading argv slot, and the variadic / full-arguments
            // adapters (rest param, `arguments[i]` tier) materialize
            // params straight off argv — a `__this` param would eat
            // argv[0]. All stay loud.
            if !mixed_calls.is_empty()
                && (closure_argc_locals.contains(name)
                    || closure_argv_locals.contains(name)
                    || fn_has_rest_param(stmts, fn_name))
            {
                continue;
            }
            patches.push(FacePatch {
                eid: init,
                fn_name: fn_name.clone(),
            });
            fnexpr_recv_faces.extend(face_eids.iter().copied());
            // EVERY promoted binding registers, not only the mixed
            // profile: the HOF lowerings detect a variable-routed
            // promoted callback through this set (a pure-face
            // binding left it empty and the loop called the RECV
            // ABI without the `__this` argv slot — `this` read the
            // element box, rotation 260). The direct-call consumer
            // (`ssa_lower_call_closure_local`) still only fires on
            // an actual bare-name call, which a pure-face profile
            // has none of.
            fnexpr_recv_locals.insert(name.clone());
        }
    }
}

/// The array HOF methods whose any-lane kernels carry the
/// receiver-first channel (knife 4). `reduce` / `reduceRight` take
/// no thisArg per §23.1.3.24 and stay out.
fn is_hof_method(name: &str) -> bool {
    matches!(
        name,
        "forEach"
            | "map"
            | "filter"
            | "every"
            | "some"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "flatMap"
    )
}

/// Every `LetDecl` matching `name`, walking fn bodies and blocks (the
/// same recursion set as [`collect_binding_names_inner`]) — knife 2's
/// uniqueness guard needs the full program-wide count, not just the
/// first hit.
fn collect_decls_by_name(stmts: &[Stmt], name: &str, out: &mut Vec<(bool, ExprId)>) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                mutable,
                name: dn,
                init,
                ..
            } if dn == name => {
                out.push((*mutable, *init));
            }
            Stmt::FnDecl { body, .. } => collect_decls_by_name(body, name, out),
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_decls_by_name(inner, name, out),
            _ => {}
        }
    }
}

/// Promote each `(closure eid, lifted fn name)` to the receiver-first
/// any shape: `__this` leaves the capture list (it is a receiver, not
/// a capture — mirror of `objlit_nominal::apply_patches`), the lifted
/// FnDecl gains a `__this: any` param right after `__env`, and the fn
/// name joins `fnexpr_recv_fns` so the construction site stamps
/// `FLAG_CLOSURE_RECV_FIRST` and receiver-aware invokers put the
/// receiver in argv[0]. Shared by this pass's fn-expr faces and
/// `objlit_nominal`'s any-lane literal members (RFC
/// 20260717-objlit-anylane-recv knife 1).
pub(crate) fn promote_recv_any(
    stmts: &mut [Stmt],
    exprs: &mut [Expr],
    patches: &[(ExprId, String)],
    fnexpr_recv_fns: &mut std::collections::HashSet<String>,
) {
    for (eid, _) in patches {
        if let Expr::Closure { captures, .. } = &mut exprs[eid.0 as usize] {
            captures.retain(|c| c != "__this");
        }
    }
    for (eid, fn_name) in patches {
        let caps: Vec<String> = match &exprs[eid.0 as usize] {
            Expr::Closure { captures, .. } => captures.clone(),
            _ => continue,
        };
        for s in stmts.iter_mut() {
            let Stmt::FnDecl { name, params, .. } = s else {
                continue;
            };
            if name != fn_name {
                continue;
            }
            if let Some(env) = params.first_mut()
                && env.name == "__env"
            {
                env.type_ann = Some(format!("__env({})", caps.join("|")));
            }
            if let Some(t) = params.iter_mut().find(|q| q.name == "__this") {
                // Knife 6 — a method-shorthand face arrives with a
                // nominal-typed `__this` (objlit_nominal's receiver);
                // the accessor invoke hands a NaN-box, so the param
                // re-anns to `any` (typeof / member reads go dynamic).
                t.type_ann = Some("any".to_string());
            } else {
                let at = usize::from(params.first().is_some_and(|q| q.name == "__env"));
                params.insert(
                    at,
                    Param {
                        name: "__this".to_string(),
                        type_ann: Some("any".to_string()),
                        default: None,
                        is_rest: false,
                    },
                );
            }
            fnexpr_recv_fns.insert(fn_name.clone());
            break;
        }
    }
}
