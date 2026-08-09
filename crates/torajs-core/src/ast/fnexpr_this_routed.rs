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
/// The construct-channel callee whitelist backing the fourth
/// receiver-safe use shape: `Reflect.construct` and the
/// `Array.from` / `Array.fromAsync` statics re-dispatched through
/// `.call` / `.apply` (their ns-static cells are recv-first, so the
/// thisArg rides argv[0] into the kernel's Construct split).
fn construct_channel_callee(exprs: &[Expr], callee: ExprId) -> bool {
    match &exprs[callee.0 as usize] {
        Expr::Member { obj, name } if name == "construct" => {
            matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Reflect")
        }
        Expr::Member { obj, name } if name == "call" || name == "apply" => {
            matches!(&exprs[obj.0 as usize], Expr::Member { obj: ns, name: m }
                if (m == "from" || m == "fromAsync")
                    && matches!(&exprs[ns.0 as usize], Expr::Ident(n) if n == "Array"))
        }
        _ => false,
    }
}

/// B6 刀 2 — an EQUALITY operand (`result.constructor === C`, the
/// t262 identity-assert spelling) is the fifth receiver-safe use
/// shape: like a `.prototype` read it enters no call lane at all —
/// the comparison consumes the cell pointer as a value. Ordering /
/// arithmetic operands stay loud (they coerce, which is observable
/// and untested here).
fn eq_operand_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    exprs
        .iter()
        .filter_map(|e| match e {
            Expr::BinOp {
                op:
                    super::BinOp::Eq
                    | super::BinOp::Neq
                    | super::BinOp::LooseEq
                    | super::BinOp::LooseNeq,
                left,
                right,
            } => Some([*left, *right]),
            _ => None,
        })
        .flatten()
        .filter(|a| matches!(&exprs[a.0 as usize], Expr::Ident(_)))
        .collect()
}

/// The sixth receiver-safe use shape (RFC 20260808-construct-channel
/// B2 knife, rotation 345): an Ident standing as an argument to a
/// program-local FnDecl whose matching param is EXPLICITLY `any`.
/// The value crosses into the any lane as a boxed cell, and every
/// any-lane call path honors the receiver channel
/// (`__torajs_any_call` / `invoke_with_this` shift argv on
/// FLAG_CLOSURE_RECV_FIRST), so no receiver-unaware call can reach
/// the promoted closure. Generic-`T` params stay loud for now —
/// monomorphization may bind a TYPED fn slot whose CallIndirect
/// bypasses the flag (the exact shifted-args silent wrong the
/// zero-alias bar forbids). The callee must be the program's only
/// FnDecl of that name (a duplicate poisons the entry), and a call
/// carrying a spread admits nothing (positions shift).
fn any_param_arg_idents(stmts: &[Stmt], exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    let mut fn_params: std::collections::HashMap<&str, Option<&[super::Param]>> =
        std::collections::HashMap::new();
    collect_fn_decl_params(stmts, &mut fn_params);
    let mut out = std::collections::HashSet::new();
    for e in exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(fname) = &exprs[callee.0 as usize] else {
            continue;
        };
        let Some(Some(params)) = fn_params.get(fname.as_str()) else {
            continue;
        };
        if args
            .iter()
            .any(|a| matches!(&exprs[a.0 as usize], Expr::Spread { .. }))
        {
            continue;
        }
        for (i, a) in args.iter().enumerate() {
            if matches!(&exprs[a.0 as usize], Expr::Ident(_))
                && let Some(p) = params.get(i)
                && !p.is_rest
                && p.type_ann.as_deref() == Some("any")
            {
                out.insert(*a);
            }
        }
    }
    out
}

/// FnDecl params by name over the whole program (fn bodies and
/// blocks recurse). A second decl of the same name poisons the
/// entry to `None` — a by-name call cannot tell which one runs.
fn collect_fn_decl_params<'a>(
    stmts: &'a [Stmt],
    out: &mut std::collections::HashMap<&'a str, Option<&'a [super::Param]>>,
) {
    for s in stmts {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                out.entry(name.as_str())
                    .and_modify(|e| *e = None)
                    .or_insert(Some(params.as_slice()));
                collect_fn_decl_params(body, out);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_fn_decl_params(inner, out),
            _ => {}
        }
    }
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
    // RFC 20260808-construct-channel B6 刀 2 — an argument handed to
    // a CONSTRUCT-CHANNEL builtin (`Reflect.construct(C, …)`,
    // `Array.from.call(C, …)` / `.apply`, fromAsync same) is the
    // fourth receiver-safe use shape: those builtins reach the
    // closure only through receiver-honoring channels (construct →
    // `invoke_with_this`; the recv-first ns-static cell prepends the
    // `.call` thisArg), so no receiver-unaware call path exists.
    // Callee whitelist, not "any builtin argument" — B-4
    // narrow-surface: each admitted callee's runtime path is audited.
    let construct_arg_idents: std::collections::HashSet<ExprId> = exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, args } if construct_channel_callee(exprs, *callee) => Some(
                args.iter()
                    .copied()
                    .filter(|a| matches!(&exprs[a.0 as usize], Expr::Ident(_))),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    // Fifth shape — equality operands (doc on the free fn below).
    let eq_operand_idents = eq_operand_idents(exprs);
    // Sixth shape — explicit-`any` param argument positions (doc on
    // the free fn above).
    let any_arg_idents = any_param_arg_idents(stmts, exprs);
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
        if !mixed_calls.iter().all(|e| {
            callee_idents.contains(e)
                || proto_read_idents.contains(e)
                || construct_arg_idents.contains(e)
                || eq_operand_idents.contains(e)
                || any_arg_idents.contains(e)
        }) {
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
            // A binding with a DIRECT CALL must not also ride a
            // boxed-argv call lane: the real-argc prepend contends
            // for the same leading argv slot, and the variadic /
            // full-arguments adapters (rest param, `arguments[i]`
            // tier) materialize params straight off argv — a
            // `__this` param would eat argv[0]. All stay loud. A
            // `.call`/`.apply` face rides the same `closure_local`
            // replay, so its binding is under the same bar even with
            // zero direct calls. A `.prototype` read is NOT under
            // the bar (species key 2): it enters no call lane at
            // all, and the store-face + argv combination is exactly
            // the escape-store profile whose adapter order the
            // rotation-338 knife fixed.
            if (mixed_calls.iter().any(|e| callee_idents.contains(e))
                || face_eids.iter().any(|e| call_faces.contains(e)))
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
