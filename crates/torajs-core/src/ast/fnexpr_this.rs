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

pub(crate) use super::fnexpr_this_faces::promote_recv_any;
use super::fnexpr_this_faces::{FacePatch, collect_face, collect_ident_face, literal_desc_faces};
use super::fnexpr_this_recvs::{
    collect_any_binding_names, collect_arraylit_binding_names, collect_gen_iter_binding_names,
    collect_mapset_binding_names, collect_props_receiver_binding_names,
};
use super::fnexpr_this_routed::promote_variable_routed;
use super::{Expr, ExprId, Stmt};

pub(crate) fn run(
    stmts: &mut [Stmt],
    exprs: &mut Vec<Expr>,
    sloppy: bool,
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
    let mut call_faces: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    collect_position_faces(
        stmts,
        exprs,
        fn_expr_exprs,
        &mut patches,
        &mut ident_cands,
        &mut call_faces,
    );
    // Rotation 375 — marked fn-exprs standing as a `throw` operand
    // (doc on the collector).
    super::fnexpr_this_faces::collect_throw_faces(stmts, stmts, exprs, fn_expr_exprs, &mut patches);
    // Seventh face position (rotation 346) — marked fn-exprs returned
    // from an objlit method/accessor body (doc on the collector).
    super::fnexpr_this_faces::collect_method_return_faces(
        stmts,
        exprs,
        fn_expr_exprs,
        objlit_method_exprs,
        &mut patches,
    );
    promote_variable_routed(
        stmts,
        exprs,
        fn_expr_exprs,
        closure_argc_locals,
        closure_argv_locals,
        ident_cands,
        &call_faces,
        &mut patches,
        fnexpr_recv_faces,
        fnexpr_recv_locals,
    );
    if patches.is_empty() {
        return;
    }
    let pairs: Vec<(ExprId, String)> = patches.into_iter().map(|p| (p.eid, p.fn_name)).collect();
    promote_recv_any(stmts, exprs, &pairs, fnexpr_recv_fns, sloppy);
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
    call_faces: &mut std::collections::HashSet<ExprId>,
) {
    let any_recvs = collect_any_binding_names(stmts, exprs);
    let gen_recvs = collect_gen_iter_binding_names(stmts, exprs);
    let arraylit_recvs = collect_arraylit_binding_names(stmts, exprs);
    let props_recvs = collect_props_receiver_binding_names(stmts, exprs);
    let mapset_recvs = collect_mapset_binding_names(stmts, exprs);
    for i in 0..exprs.len() {
        if let Expr::Assign { target, value } = &exprs[i] {
            collect_store_face(
                stmts,
                exprs,
                fn_expr_exprs,
                &props_recvs,
                *target,
                *value,
                patches,
                ident_cands,
            );
            continue;
        }
        // Rotation 375 — the NewDynamic inline-callee face:
        // `new (function () { …this… })(…)`. Zero aliases by
        // construction — the closure value exists only as this
        // construct's callee — and the runtime construct channel is
        // receiver-honoring end to end (rotation 345 knife 5:
        // `__torajs_anyv_construct`'s plain-fn kernel invokes through
        // `invoke_with_this` with the freshly allocated `this`, which
        // shifts argv on FLAG_CLOSURE_RECV_FIRST). The Ident-callee
        // spelling already rides `construct_channel_arg_idents`; this
        // arm is its inline twin.
        if let Expr::NewDynamic { callee, .. } = &exprs[i] {
            collect_face(stmts, exprs, *callee, fn_expr_exprs, patches);
            continue;
        }
        let Expr::Call { callee, args } = &exprs[i] else {
            continue;
        };
        match &exprs[callee.0 as usize] {
            // Rotation 328 — the IIFE arm: the callee IS the marked
            // fn-expr (`(function () { …this… })()` / `}())`). Zero
            // aliases by construction — the closure value exists only
            // as this call's callee — and the receiverless call site
            // seeds a boxed `undefined` into the promoted `__this`
            // slot (fn_indirect's promoted-callee lane, §10.2.1.2
            // strict call-site `this`, the blade-1 framing).
            Expr::Closure { .. } => {
                collect_face(stmts, exprs, *callee, fn_expr_exprs, patches);
            }
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
            // RFC 20260808-json-parse-any blade 4 — `JSON.parse(text,
            // <fn-expr>)`: the reviver slot. §25.5.1.1 calls it with
            // the holder as `this`; the internalize kernel dispatches
            // through `invoke_with_this`, which reads the closure's
            // RECV flag and seeds the holder into the promoted
            // `__this` slot. Inline fn-expr only — zero aliases.
            Expr::Member { obj, name } if name == "parse" => {
                if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "JSON") {
                    continue;
                }
                if let Some(rev) = args.get(1) {
                    collect_face(stmts, exprs, *rev, fn_expr_exprs, patches);
                    collect_ident_face(exprs, *rev, ident_cands);
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
            // Rotation 261 — `f.call(thisArg, ...)` / `f.apply(
            // thisArg, [args])` on a bare-name binding: §20.2.3.3 /
            // §20.2.3.1 supply an EXPLICIT this, so the callee-obj
            // position is receiver-correct by construction — the
            // fn-value lowering boxes thisArg into the promoted
            // `__this` argv slot instead of the no-this drop. The
            // Ident joins `call_faces` too: this use shape replays
            // through `closure_local`, whose variadic / real-argc
            // lanes have no `__this` slot, so those bindings keep
            // the loud reject (same argv-contention bar as the
            // knife-2W mixed profile).
            Expr::Member { obj, name } if name == "call" || name == "apply" => {
                // An INLINE fn-expr callee (`(function () { …this…
                // }).apply(obj)`) promotes directly — zero aliases
                // by construction, the call is its only consumer.
                collect_face(stmts, exprs, *obj, fn_expr_exprs, patches);
                if matches!(&exprs[obj.0 as usize], Expr::Ident(_)) {
                    collect_ident_face(exprs, *obj, ident_cands);
                    call_faces.insert(*obj);
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
            // ArrayLit init) promotes for the inlined-loop shapes
            // that thread a thisArg: the trio (`forEach` / `map` /
            // `filter` — `ssa_lower_call_arr_ho`), the predicate
            // family (`find*` / `some` / `every` — rotation 261),
            // and `flatMap` (rotation 286 — its inlined loop now
            // mirrors the same knife-4 protocol); other receivers
            // keep today's loud reject.
            // Iterator-helper cb over a syntactically-certain
            // generator-object receiver (`let it = g(); it.every(fn)`)
            // — the eager and lazy kernels both read the closure flag
            // and seed the receiver slot undefined (§27.1.4's
            // Call(cb, undefined)). `reduce` has a cb here (unlike
            // the array family, whose reduce keeps its loud reject).
            Expr::Member { obj, name }
                if (is_hof_method(name) || name == "reduce")
                    && matches!(&exprs[obj.0 as usize], Expr::Ident(n) if gen_recvs.contains(n)) =>
            {
                if let Some(cb) = args.first() {
                    collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
                }
            }
            Expr::Member { obj, name } if is_hof_method(name) => {
                collect_hof_cb_face(
                    stmts,
                    exprs,
                    fn_expr_exprs,
                    &any_recvs,
                    &arraylit_recvs,
                    &mapset_recvs,
                    *obj,
                    name,
                    args,
                    patches,
                    ident_cands,
                );
            }
            _ => {}
        }
    }
}

/// Member-store face — `anyRecv.m = <fn-expr>` (expando method
/// on a wrapper / dynobj receiver). The stored closure is only
/// reachable back through the receiver's props, so every call
/// rides the runtime any-method dispatch, which reads the
/// FLAG_CLOSURE_n header bit and seeds the receiver —
/// zero-alias by construction for a literal fn-expr RHS. An
/// Ident RHS (`o.m = f`) is a face POSITION for knife-2's
/// use-shape analysis: the binding promotes only when its
/// remaining uses are all face reads / direct calls, same bar
/// as every other variable-routed face. (The Index-target
/// twin `o["k"] = fn` rides this arm through the parser's
/// Member desugar for STRING-literal keys; a computed key —
/// `o[Symbol.match] = fn`, `o[k] = fn` — stays Expr::Index
/// and matches the second target pattern: the stored value
/// reaches calls only through the same runtime keyed
/// dispatch, so the face bar is identical.)
///
/// Admitted store receivers:
/// * a runtime-props binding Ident — every declaration of the name
///   is `: any` / `T[]`-annotated, an unannotated `{}` init, or an
///   unannotated array literal (the species-key-2 merged predicate;
///   an array's expando members live in the arrprops bag, the same
///   runtime keyed dispatch the dynobj lane reads);
/// * knife 7 — `F.prototype.m = <fn-expr>` / `F.prototype[k] =
///   <fn-expr>` (the test262 fn-constructor idiom). The prototype
///   object is a runtime dynobj, so the stored closure is reachable
///   only through instances' proto chains and the runtime keyed
///   dispatch. By this pass a fn-decl name in receiver position is
///   already a `__forward_*` Closure wrapper
///   (`synthesize_fn_to_closure_forwarders`), so both spellings of
///   "some named fn's .prototype" admit;
/// * RFC 20260808-construct-channel B2 —
///   `a.constructor[Symbol.species] = <fn>` / `a.constructor.k =
///   <fn>` where `a` is a runtime-props binding (same merged
///   predicate, mutability irrelevant). `.constructor` on those
///   shapes is never a typed struct slot — the stored closure is
///   reachable only through the runtime keyed dispatch
///   (ArraySpeciesCreate reads it back through the arrprops bag).
///   Other member names on these roots stay loud until a consumer
///   shape needs them.
fn collect_store_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    props_recvs: &std::collections::HashSet<String>,
    target: ExprId,
    value: ExprId,
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    let store_recv = match &exprs[target.0 as usize] {
        Expr::Member { obj, .. } => Some(*obj),
        Expr::Index { obj, .. } => Some(*obj),
        _ => None,
    };
    let admits = store_recv.is_some_and(|obj| match &exprs[obj.0 as usize] {
        Expr::Ident(n) => props_recvs.contains(n),
        Expr::Member {
            obj: pobj,
            name: pname,
        } => {
            (pname == "prototype"
                && matches!(
                    &exprs[pobj.0 as usize],
                    Expr::Ident(_) | Expr::Closure { .. }
                ))
                || (pname == "constructor"
                    && matches!(&exprs[pobj.0 as usize], Expr::Ident(n)
                        if props_recvs.contains(n)))
        }
        _ => false,
    });
    if admits {
        collect_face(stmts, exprs, value, fn_expr_exprs, patches);
        collect_ident_face(exprs, value, ident_cands);
    }
}

/// Knife 4 — the non-generator HOF callback arm: any-lane receivers
/// (the kernels read the closure flag and seed argv[0] with the
/// thisArg or the buffer's undefined padding — §23.1.3's T), typed
/// ArrayLit-init receivers for the inlined-loop shapes that thread a
/// thisArg (the trio / predicate family / `flatMap`), Map / Set
/// receivers for `forEach` only, and the INLINE array-literal
/// receiver (`[1,2].forEach(<fn-expr>, thisArg)` — the
/// arraylit_recvs shape without the binding hop). Other receivers
/// keep today's loud reject. A variable-routed callback rides knife
/// 2's zero-alias profile; the HOF lowerings detect the promoted
/// binding through `fnexpr_recv_locals` (the Closure-shape detection
/// alone ran the loop with shifted args — probed rotation 260, fixed
/// same rotation).
#[allow(clippy::too_many_arguments)]
fn collect_hof_cb_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    any_recvs: &std::collections::HashSet<String>,
    arraylit_recvs: &std::collections::HashSet<String>,
    mapset_recvs: &std::collections::HashSet<String>,
    obj: ExprId,
    name: &str,
    args: &[ExprId],
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    let mapset_ok = name == "forEach";
    let inline_arraylit = matches!(&exprs[obj.0 as usize], Expr::Array(_));
    if !inline_arraylit
        && !matches!(&exprs[obj.0 as usize], Expr::Ident(n)
        if any_recvs.contains(n)
            || arraylit_recvs.contains(n)
            || (mapset_ok && mapset_recvs.contains(n)))
    {
        return;
    }
    if let Some(cb) = args.first() {
        collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
        collect_ident_face(exprs, *cb, ident_cands);
    }
}

/// The array HOF methods whose any-lane kernels carry the
/// receiver-first channel (knife 4). `reduce` / `reduceRight` take
/// no thisArg per §23.1.3.24 and stay out. Shared with the named-fn
/// callback mirror ([`super::namedfn_recv_cb`]).
pub(super) fn is_hof_method(name: &str) -> bool {
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
