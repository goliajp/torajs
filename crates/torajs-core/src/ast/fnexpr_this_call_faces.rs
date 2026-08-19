//! The CALL-position face arms of `fnexpr_this`'s walk — carved out
//! in rotation 447 (the parent stood at 490 of the 500-line limit
//! with the Symbol.replace arm landed; the next face knife had no
//! room). Verbatim move: the parent keeps the POSITION walk (which
//! arena node is a face position), this sibling answers what a
//! call's CALLEE shape contributes — the knives' arms in the order
//! they were added, plus the knife-4 HOF collector and the shared
//! `is_hof_method` table (re-exported through the parent so
//! `namedfn_recv_cb`'s import face is unchanged).

use super::fnexpr_this_faces::{FacePatch, collect_face, collect_ident_face, literal_desc_faces};
use super::{Expr, ExprId, Stmt};

/// The CALL-position arms of the walk above — one match over the
/// callee's shape, in the order the knives added them.
///
/// Split out of the walk in rotation 399: `collect_position_faces`
/// stood at 195 of the 200-line limit and the next face position (the
/// array-literal element, registered 399-02) adds about eight lines to
/// it. Same先移后加 the file-size table asks for, one level down.
///
/// **Arm ORDER is load-bearing** — several arms match on a member NAME
/// with no receiver test of their own and rely on the earlier arms
/// having claimed theirs, so the block moves verbatim rather than
/// being regrouped.
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_call_position_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    any_recvs: &std::collections::HashSet<String>,
    gen_recvs: &std::collections::HashSet<String>,
    arraylit_recvs: &std::collections::HashSet<String>,
    mapset_recvs: &std::collections::HashSet<String>,
    strwrapper_recvs: &std::collections::HashSet<String>,
    callee: ExprId,
    args: &[ExprId],
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
    call_faces: &mut std::collections::HashSet<ExprId>,
) {
    match &exprs[callee.0 as usize] {
        // Rotation 328 — the IIFE arm: the callee IS the marked
        // fn-expr (`(function () { …this… })()` / `}())`). Zero
        // aliases by construction — the closure value exists only
        // as this call's callee — and the receiverless call site
        // seeds a boxed `undefined` into the promoted `__this`
        // slot (fn_indirect's promoted-callee lane, §10.2.1.2
        // strict call-site `this`, the blade-1 framing).
        Expr::Closure { .. } => {
            collect_face(stmts, exprs, callee, fn_expr_exprs, patches);
        }
        // `recv.__defineGetter__(k, face)` / `__defineSetter__`.
        Expr::Member { name, .. } if name == "__defineGetter__" || name == "__defineSetter__" => {
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
                return;
            }
            let Some(desc) = args.get(2) else { return };
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
                return;
            }
            let Some(props) = args.get(1) else { return };
            let Expr::ObjectLit { fields } = &exprs[props.0 as usize] else {
                return;
            };
            let descs: Vec<ExprId> = fields.iter().map(|(_, deid)| *deid).collect();
            for desc in descs {
                for face in literal_desc_faces(exprs, desc) {
                    collect_face(stmts, exprs, face, fn_expr_exprs, patches);
                    collect_ident_face(exprs, face, ident_cands);
                }
            }
        }
        // Builtin callback-slot arms — bodies (and their receiver
        // checks, fall-through-safe: no later arm matches these
        // member names) live in sibling `fnexpr_this_cb_slots.rs`
        // (file-size split, rotation 375).
        Expr::Member { obj, name } if name == "parse" || name == "stringify" => {
            super::fnexpr_this_cb_slots::collect_json_face(
                stmts,
                exprs,
                fn_expr_exprs,
                *obj,
                args,
                patches,
                ident_cands,
            );
        }
        Expr::Member { obj, name } if name == "replace" || name == "replaceAll" => {
            super::fnexpr_this_cb_slots::collect_replace_face(
                stmts,
                exprs,
                fn_expr_exprs,
                *obj,
                args,
                &strwrapper_recvs,
                patches,
                ident_cands,
            );
        }
        Expr::Member { obj, name } if name == "from" || name == "fromAsync" => {
            super::fnexpr_this_cb_slots::collect_array_from_face(
                stmts,
                exprs,
                fn_expr_exprs,
                *obj,
                args,
                patches,
                ident_cands,
            );
        }
        // Rotation 261 — `f.call` / `f.apply` faces (doc on the
        // collector in sibling `fnexpr_this_cb_slots.rs`).
        Expr::Member { obj, name } if name == "call" || name == "apply" => {
            super::fnexpr_this_cb_slots::collect_call_apply_face(
                stmts,
                exprs,
                fn_expr_exprs,
                *obj,
                patches,
                ident_cands,
                call_faces,
            );
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
        // Rotation 447 — the `@@replace` protocol spelling (doc on
        // the collector in sibling `fnexpr_this_cb_slots.rs`).
        Expr::Index { obj, index } => {
            super::fnexpr_this_cb_slots::collect_symbol_replace_face(
                stmts,
                exprs,
                fn_expr_exprs,
                *obj,
                *index,
                args,
                patches,
                ident_cands,
            );
        }
        _ => {}
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
