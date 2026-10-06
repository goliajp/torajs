//! Face collectors for the call-position cascade — the BODIES of
//! `fnexpr_this_call_faces`'s arms, carved out of them under the
//! file-size hard limit (rotation 375 moved the builtin
//! callback-slot arms when the replace arm pushed the file to 517
//! and the walk fn to 226; rotation 632 moved the
//! property-definition arms when the same fn reached 199 and the
//! next arm had nowhere to go). Verbatim moves; each arm's receiver
//! check moved from the match guard into the collector body, which
//! is fall-through-safe because no later cascade arm matches the
//! same member name.

use super::fnexpr_this_faces::{FacePatch, collect_face, collect_ident_face, literal_desc_faces};
use super::{Expr, ExprId, Stmt};

/// RFC 20260808-json-parse-any blade 4 — `JSON.parse(text,
/// <fn-expr>)`: the reviver slot. §25.5.1.1 calls it with the holder
/// as `this`; the internalize kernel dispatches through
/// `invoke_with_this`, which reads the closure's RECV flag and seeds
/// the holder into the promoted `__this` slot. Inline fn-expr only —
/// zero aliases.
///
/// `JSON.stringify(value, <fn-expr>)`'s replacer slot is the same
/// shape and shares this collector: §25.5.2.2 step 3 calls it with
/// the holder as `this` (the synthetic `{ "": value }` wrapper at the
/// root), through the same `invoke_with_this`. Every admitted
/// spelling routes to that kernel — the lowering's
/// `serves_as_replacer` takes a `Function` or an `Any` out of the
/// static unfold — so the promotion can never outrun the seeding.
pub(super) fn collect_json_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    args: &[ExprId],
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "JSON") {
        return;
    }
    if let Some(rev) = args.get(1) {
        collect_face(stmts, exprs, *rev, fn_expr_exprs, patches);
        collect_ident_face(exprs, *rev, ident_cands);
    }
}

/// Rotation 375 — string-pattern `.replace` / `.replaceAll`
/// functional replaceValue over a STRING-LITERAL receiver
/// (`"ab".replace("b", <fn-expr>)`, the function-code 10.4.3
/// family). The literal receiver + literal pattern pin the lowering
/// to the str replace_fn lane (`ssa_lower_str_replace_fn`'s
/// String/Null/Undefined pattern gate), whose kernel reads the
/// closure's receiver-first flag and seeds argv[0] undefined
/// (§22.1.3.18 step 10's Call(replaceValue, undefined, «…»)). A
/// non-literal receiver or a regex pattern keeps the loud reject —
/// the regex replacer lane dispatches by typed transmute and has no
/// receiver slot (L3b 375-01). The inline fn-expr and the
/// variable-routed Ident (knife 2's zero-alias profile, rotation
/// 446) both admit. The IIFE-return spelling was tried and REVERTED
/// in rotation 375 — the checker typed the IIFE call's result as
/// non-Function, the lowering fell to the ToString replaceValue
/// path, and the promote turned a loud reject into source-text
/// splicing — and rotation 446 unblocked it at the root: an `any`
/// replaceValue now answers §22.1.3.19 step 5's IsCallable at
/// runtime (`__torajs_str_replace_any_repl`), so the fn-return
/// faces and this slot compose instead of colliding.
///
/// Rotation 447 — an inline `new String(...)` receiver admits with
/// NO pattern gate: the wrapper mint checker-types `Any`
/// (`check_type_of_new.rs`), so the whole call rides the runtime
/// any-dispatch, where BOTH pattern lanes now serve a functional
/// replaceValue through receiver-flag-aware kernels (the literal
/// glue reads FLAG_CLOSURE_RECV_FIRST; the regex lane's boxed
/// kernel does the same with the pattern's own capture count).
/// A variable-routed wrapper receiver admits on the same argument
/// through `strwrapper_recvs` (the syntactically-certain census in
/// `fnexpr_this_recvs.rs` — every reaching value is a mint).
/// Fail-safe: this shape was an unconditional unclaimed loud
/// reject before, so admitting it can only turn a compile error
/// into the spec answer.
// the call-site pieces plus the walk output vectors
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_replace_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    args: &[ExprId],
    strwrapper_recvs: &std::collections::HashSet<String>,
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    let wrapper_recv = matches!(
        &exprs[obj.0 as usize],
        Expr::New { class_name, .. } if class_name == "String"
    ) || matches!(
        &exprs[obj.0 as usize],
        Expr::Ident(n) if strwrapper_recvs.contains(n)
    );
    if !wrapper_recv {
        if !matches!(&exprs[obj.0 as usize], Expr::String(_)) {
            return;
        }
        if !args
            .first()
            .is_some_and(|p| is_str_pattern_literal(exprs, *p))
        {
            return;
        }
    }
    if let Some(cb) = args.get(1) {
        collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
        collect_ident_face(exprs, *cb, ident_cands);
    }
}

/// `true` when the replace-family pattern argument is a shape the
/// str replace_fn lane accepts (mirror of
/// `ssa_lower_str_replace_fn`'s String / Null / Undefined checker
/// gate) — anything else may route to the regex replacer lane,
/// whose dispatch has no receiver slot.
fn is_str_pattern_literal(exprs: &[Expr], p: ExprId) -> bool {
    matches!(&exprs[p.0 as usize], Expr::String(_) | Expr::Null)
        || matches!(&exprs[p.0 as usize], Expr::Ident(n) if n == "undefined")
}

/// Rotation 261 — `f.call(thisArg, ...)` / `f.apply(thisArg,
/// [args])` on a bare-name binding: §20.2.3.3 / §20.2.3.1 supply an
/// EXPLICIT this, so the callee-obj position is receiver-correct by
/// construction — the fn-value lowering boxes thisArg into the
/// promoted `__this` argv slot instead of the no-this drop. The
/// Ident joins `call_faces` too: this use shape replays through
/// `closure_local`, whose variadic / real-argc lanes have no
/// `__this` slot, so those bindings keep the loud reject (same
/// argv-contention bar as the knife-2W mixed profile). An INLINE
/// fn-expr callee (`(function () { …this… }).apply(obj)`) promotes
/// directly — zero aliases by construction, the call is its only
/// consumer.
/// Which node is the FACE of a `call` / `apply` call — the member's
/// object for `f.call(o)` / `f.apply(o, …)`, argument 0 for §28.1.1
/// `Reflect.apply(target, thisArg, argumentsList)`.
///
/// Both spellings reach the target through
/// `method_call_closure_apply_like::apply_list`, one dispatch that
/// shifts argv on FLAG_CLOSURE_RECV_FIRST, so the promoted body's
/// `__this` is exactly the thisArg either way. Only the node holding
/// the function differs — and reading the member's object for
/// `Reflect.apply` would collect `Reflect` itself, which is how that
/// spelling stayed outside every admitted shape until rotation 632.
pub(super) fn call_apply_face_node(
    exprs: &[Expr],
    obj: ExprId,
    name: &str,
    args: &[ExprId],
) -> Option<ExprId> {
    if name == "apply" && matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Reflect") {
        return args.first().copied();
    }
    Some(obj)
}

pub(super) fn collect_call_apply_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
    call_faces: &mut std::collections::HashSet<ExprId>,
) {
    // An `as` wrapper changes nothing about who supplies the
    // receiver: `(f as any).call(o)` is still §20.2.3.3 handing `o`
    // to `f`, and the any-lane method dispatch it lowers through
    // shifts argv on FLAG_CLOSURE_RECV_FIRST exactly like the static
    // lane does. Without the peel the binding saw an unrecognized
    // use shape, declined promotion, and its body's `this` capture
    // then had nothing to bind to ("closure `__closure_N` references
    // unknown identifier `__this`") — while the same call written
    // without the cast worked.
    let obj = super::fnexpr_this_names::peel_as(exprs, obj);
    collect_face(stmts, exprs, obj, fn_expr_exprs, patches);
    if matches!(&exprs[obj.0 as usize], Expr::Ident(_)) {
        collect_ident_face(exprs, obj, ident_cands);
        call_faces.insert(obj);
    }
}

/// Rotation 260 — `Array.from(iterable, <fn-expr>, thisArg?)`: the
/// static mapFn slot. Only the INLINE fn-expr promotes (zero aliases
/// by construction); the from lowering's map loop threads the boxed
/// thisArg ahead of (elem, i) exactly like the trio kernels.
/// `Array.fromAsync` is not here: it is self-hosted (RFC 20260914),
/// and its helper hands the thisArg to `mapfn.call` itself.
///
/// Rotation 628 — the receiver may also be an any-lane value: a
/// `: any` binding / parameter (`TA.from(src, mapfn, thisArg)`,
/// test262's `testWithTypedArrayConstructors` body) or an `as any`
/// cast (`(Uint8Array as any).from(…)`). Such a call rides the
/// runtime ctor-static dispatch, whose `%TypedArray%.from` /
/// `Array.from` kernels both hand `thisArg` to every mapfn call on
/// the receiver channel — the same seed the `Array` spelling gets.
/// A TYPED TypedArray constructor ident (`Uint8Array.from(src,
/// mapfn, thisArg)`) is the same lane one step earlier: its lowering
/// boxes the ctor cell and rides the any-method call
/// (`ssa_lower_call_typedarray_static`).
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_array_from_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    any_recvs: &std::collections::HashSet<String>,
    obj: ExprId,
    args: &[ExprId],
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    let recv = super::fnexpr_this_faces::peel_any_cast(exprs, obj);
    let any_lane = recv != obj
        || matches!(&exprs[recv.0 as usize], Expr::Ident(n) if any_recvs.contains(n)
            || crate::ssa_lower_call_typedarray::bytes_per_element(n).is_some());
    if !any_lane && !matches!(&exprs[recv.0 as usize], Expr::Ident(n) if n == "Array") {
        return;
    }
    if let Some(cb) = args.get(1) {
        collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
        collect_ident_face(exprs, *cb, ident_cands);
    }
}

/// Rotation 447 — `<regex-literal>[Symbol.replace](str, <fn-expr>)`:
/// the §22.2.6.11 protocol spelling of the replace cb slot. The
/// computed symbol call lowers through the runtime index-method
/// dispatch, whose symbol probe reifies the RegExp proto's
/// `@@replace` cell and re-enters the by-mid dispatch — the regexp
/// arm flips the operands back into the Str home
/// (`method_call_regexp.rs`), which is the SAME receiver-flag-aware
/// functional kernel chain the `.replace` spelling rides (the regex
/// lane's boxed kernel reads `FLAG_CLOSURE_RECV_FIRST` with the
/// pattern's own capture count). Inline regex-literal receivers
/// only — every other computed-callee shape keeps the loud reject.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_symbol_replace_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    index: ExprId,
    args: &[ExprId],
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    if !matches!(&exprs[index.0 as usize], Expr::Member { obj: s, name }
        if name == "replace" && matches!(&exprs[s.0 as usize], Expr::Ident(n) if n == "Symbol"))
    {
        return;
    }
    if !matches!(&exprs[obj.0 as usize], Expr::Regex { .. }) {
        return;
    }
    if let Some(cb) = args.get(1) {
        collect_face(stmts, exprs, *cb, fn_expr_exprs, patches);
        collect_ident_face(exprs, *cb, ident_cands);
    }
}

/// The property-definition faces — one collector for the three
/// spellings that hand a function into a property descriptor. Split
/// from the cascade in rotation 632: the walk fn stood at 199 lines
/// and the next arm had nowhere to go.
///
/// * `recv.__defineGetter__(k, face)` / `__defineSetter__` — the
///   face is argument 1 and the receiver is anything.
/// * `Object.defineProperty(o, k, { get: face, set: face })` — the
///   descriptor must be an INLINE object literal; a variable-routed
///   descriptor aliases its faces (knife 2). `Reflect.defineProperty`
///   is the same descriptor in the same position (§28.1.2 calls the
///   same [[DefineOwnProperty]]), so its faces are claimed alike.
/// * `Object.defineProperties(o, { k: { get: face } })` /
///   `Object.create(proto, { k: { get: face } })` — the nested
///   descriptor-of-descriptors form (knife 5). Both layers must be
///   inline object literals so every face keeps zero aliases, the
///   same narrow-surface bar the flat descriptor carries.
///
/// The two `Object.*` families answer nothing when the receiver is
/// not the `Object` ident (`Reflect` only for `defineProperty`), which is what the guard did in the
/// cascade: no later arm matches these member names, so declining
/// here and ending the walk are the same thing.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_property_definition_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    obj: ExprId,
    name: &str,
    args: &[ExprId],
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    let take = |face: ExprId, patches: &mut Vec<FacePatch>, cands: &mut Vec<(String, ExprId)>| {
        collect_face(stmts, exprs, face, fn_expr_exprs, patches);
        collect_ident_face(exprs, face, cands);
    };
    if name == "__defineGetter__" || name == "__defineSetter__" {
        if let Some(&face) = args.get(1) {
            take(face, patches, ident_cands);
        }
        return;
    }
    let ns = match &exprs[obj.0 as usize] {
        Expr::Ident(n) if n == "Object" || n == "Reflect" => n.as_str(),
        _ => return,
    };
    if name == "defineProperty" {
        let Some(&desc) = args.get(2) else { return };
        for face in literal_desc_faces(exprs, desc) {
            take(face, patches, ident_cands);
        }
        return;
    }
    if ns == "Reflect" {
        return;
    }
    let Some(&props) = args.get(1) else { return };
    let Expr::ObjectLit { fields } = &exprs[props.0 as usize] else {
        return;
    };
    let descs: Vec<ExprId> = fields.iter().map(|(_, deid)| *deid).collect();
    for desc in descs {
        for face in literal_desc_faces(exprs, desc) {
            take(face, patches, ident_cands);
        }
    }
}
