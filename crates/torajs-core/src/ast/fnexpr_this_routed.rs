//! Knife-2 variable-routed face promotion — carved out of
//! `fnexpr_this.rs` when knife 7 (prototype-store faces) pushed it
//! past the 500-line hard limit. Verbatim move; the position walk
//! stays in the parent, which hands its Ident candidates here.

use super::fnexpr_this_faces::FacePatch;
use super::fnexpr_this_recvs::{
    collect_decls_by_name, collect_this_fnexpr_decl_names, fn_has_rest_param,
    name_shadowed_elsewhere,
};
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
    // Rotation 328 — the ZERO-FACE all-direct-call profile:
    // `var f = function () { …this… }; f();` never stands in a face
    // position, but every guard below already covers it — the
    // use-vs-face parity degenerates to "every use is a direct-call
    // callee", and knife-2W cut 2's `closure_local` lane seeds a boxed
    // `undefined` into the promoted `__this` slot at each such call
    // (§10.2.1.2 strict call-site `this`, the `this_param.rs` blade-1
    // framing — matching bun's module-goal answer). Injecting the
    // binding with an empty face list is the whole knife; any use
    // shape besides a direct call still rejects in the parity check.
    let mut call_only: Vec<String> = Vec::new();
    collect_this_fnexpr_decl_names(stmts, exprs, fn_expr_exprs, &mut call_only);
    for name in call_only {
        faces_by_name.entry(name).or_default();
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
    // RFC 20260808-construct-channel B2 — `Ctor.prototype` reads are
    // the third receiver-safe use shape: a member READ never calls
    // the closure, and `.prototype` on a promoted plain fn-expr
    // answers the canonical fnprops cell the construct kernel links
    // (§10.2.5 fn_prototype_pair — the create-species assert shape,
    // `Object.getPrototypeOf(thisValue) === Ctor.prototype`). Only
    // `.prototype` admits: `.call` / `.apply` reads feed an immediate
    // call that rides its own `call_faces` replay bar, and any other
    // member read stays loud until a consumer shape needs it.
    let proto_read_idents: std::collections::HashSet<ExprId> = exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Member { obj, name }
                if name == "prototype" && matches!(&exprs[obj.0 as usize], Expr::Ident(_)) =>
            {
                Some(*obj)
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
        if !mixed_calls
            .iter()
            .all(|e| callee_idents.contains(e) || proto_read_idents.contains(e))
        {
            continue;
        }
        let mut decls: Vec<(bool, ExprId)> = Vec::new();
        collect_decls_by_name(stmts, name, &mut decls);
        if decls.len() != 1 {
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
        let (_mutable, init) = decls[0];
        if fn_expr_exprs.contains(&init)
            && let Expr::Closure { fn_name, captures } = &exprs[init.0 as usize]
            && captures.iter().any(|c| c == "__this")
        {
            if name_shadowed_elsewhere(stmts, name) {
                continue;
            }
            // A mixed binding must not also ride a boxed-argv call
            // lane: the real-argc prepend contends for the same
            // leading argv slot, and the variadic / full-arguments
            // adapters (rest param, `arguments[i]` tier) materialize
            // params straight off argv — a `__this` param would eat
            // argv[0]. All stay loud. A `.call`/`.apply` face rides
            // the same `closure_local` replay, so its binding is
            // under the same bar even with zero direct calls.
            if (!mixed_calls.is_empty() || face_eids.iter().any(|e| call_faces.contains(e)))
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
