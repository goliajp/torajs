//! RFC 20260817-fnsig-reabstraction-thunk — sig-bridging thunks for
//! the bare-FnSig call lane.
//!
//! A top-level head-less FnDecl passed into a fn-typed param travels
//! as a BARE function pointer, and the receiving call site fires a
//! `call_indirect` shaped by the PARAM's annotation — not by the
//! callee's declared signature. When the two disagree, the callee
//! reads argument registers the caller never filled (a declared
//! defaulted param past the formal arity) or reads a raw typed value
//! through an Any-repr slot (a widened prefix position): silent
//! garbage, no argc channel, no §10.2.1.4 normalization. The
//! checker's S2 call-face admit (RFC 20260810) deliberately accepts
//! these shapes because the env-first closure lane delivers
//! `undefined` into unpassed Any slots — a guarantee the bare-FnSig
//! lane never had.
//!
//! The fix keeps the bare lane exactly as fast as it is — a sig-exact
//! function still travels as its own address — and reabstracts the
//! mismatched case, the way Swift bridges calling-convention
//! mismatches with reabstraction thunks:
//!
//! ```text
//! function gb(p = mk()) {...}          // materialized: gb(p: any)
//! function callit(f: () => void) {...}
//! callit(gb)   →   callit(__sigthunk_gb_0)
//! // synthesized, head-less, sig-exact against the formal:
//! function __sigthunk_gb_0(): void { gb(undefined); }
//! ```
//!
//! The explicit `undefined` overflow arguments make the callee's
//! materialized body guards (`if (p === undefined) p = <default>`)
//! fire per §10.2.1.4, and a widened prefix position rides the
//! ordinary direct-call argument conversion (number → any boxing)
//! inside the thunk body. Runs at the very end of the desugar chain:
//! after `materialize_expr_defaults` (the target's params carry
//! their final `any` spellings), after the forwarder passes (whose
//! axes own the store-site positions; this pass only touches
//! call-argument positions), and after `apply_default_args` (the
//! thunk body passes every argument explicitly, so the pad table is
//! not involved).
//!
//! First-cut admission is deliberately conservative — see
//! [`thunk_plan`]. Excluded shapes (recorded in the RFC): a formal
//! `any` position receiving a TYPED actual param (the thunk body
//! would need an any→typed narrowing the checker refuses), dynamic
//! fn values (the cell / boxed channels own those), and
//! assignment-position slots (`check_assignable` keeps the strict
//! face on purpose, rotation 355).

use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

/// A param annotation as a comparable spelling: absent means `any`
/// (the checker's default for an unannotated param).
fn norm_ann(ann: Option<&str>) -> &str {
    match ann {
        Some(a) => {
            let t = a.trim();
            if t.is_empty() { "any" } else { t }
        }
        None => "any",
    }
}

/// One top-level head-less FnDecl's face, snapshotted for the
/// immutable collect phase.
struct FnFace {
    param_anns: Vec<String>,
    ret: String,
    eligible_target: bool,
}

/// Snapshot every top-level FnDecl whose declared face this pass can
/// reason about: head-less (no `__`-prefixed synthetic param — that
/// excludes lifted closures, argv-face bodies, promoted `__this`
/// carriers) and un-shadowed. `eligible_target` additionally demands
/// no rest param and no generator factory — a callee we substitute
/// must be replayable as a plain positional direct call.
fn snapshot_faces(ast: &Ast) -> HashMap<String, FnFace> {
    let mut faces = HashMap::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            is_generator,
            type_params,
            ..
        } = s
        else {
            continue;
        };
        if params.iter().any(|p| p.name.starts_with("__")) {
            continue;
        }
        if super::fnexpr_this_names::name_shadowed_elsewhere(&ast.stmts, name) {
            continue;
        }
        let mut lets = Vec::new();
        super::fnexpr_this_names::collect_decls_by_name(&ast.stmts, name, &mut lets);
        if !lets.is_empty() {
            continue;
        }
        faces.insert(
            name.clone(),
            FnFace {
                param_anns: params
                    .iter()
                    .map(|p| norm_ann(p.type_ann.as_deref()).to_string())
                    .collect(),
                ret: return_type
                    .as_deref()
                    .map(|r| r.trim().to_string())
                    .unwrap_or_else(|| "void".to_string()),
                eligible_target: !is_generator
                    && type_params.is_empty()
                    && !params.iter().any(|p| p.is_rest),
            },
        );
    }
    faces
}

/// The formal face a fn-typed param annotation spells, or None when
/// the spelling is not a fn-type / not splittable.
fn formal_face(ann: Option<&str>) -> Option<(Vec<String>, String)> {
    let canon = crate::num_width::fn_type_canon(ann?)?;
    let (ps, ret) = crate::num_width::split_fn_type(&canon)?;
    let ps: Vec<String> = ps
        .into_iter()
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    Some((ps, ret.to_string()))
}

/// Decide whether passing `target` into a slot spelled
/// `(formal_ps) => formal_ret` needs — and safely admits — a thunk.
///
/// Needs one iff the two faces are not spelling-identical. Admits
/// one iff every position where they DISAGREE, and every declared
/// position past the formal arity, is `any` on the target's side
/// (those slots receive the undefined box / the boxed widening the
/// direct call in the thunk body already knows how to produce), and
/// the return faces line up (a void/undefined formal discards; any
/// other formal requires equal-or-any against the target's return —
/// mirroring the S2 admit this pass exists to make true).
fn thunk_plan(target: &FnFace, formal_ps: &[String], formal_ret: &str) -> bool {
    if !target.eligible_target {
        return false;
    }
    let exact = target.param_anns.len() == formal_ps.len()
        && target
            .param_anns
            .iter()
            .zip(formal_ps.iter())
            .all(|(a, f)| a == f)
        && target.ret == *formal_ret;
    if exact {
        return false;
    }
    let params_safe = target.param_anns.iter().enumerate().all(|(i, a)| {
        if let Some(f) = formal_ps.get(i) {
            a == f || a == "any"
        } else {
            a == "any"
        }
    });
    let ret_safe = formal_ret == "void"
        || formal_ret == "undefined"
        || formal_ret == target.ret
        || formal_ret == "any"
        || target.ret == "any";
    params_safe && ret_safe
}

/// See module doc. Two-phase over the expr arena (the
/// `materialize_expr_defaults` shape): collect every admitted
/// (call-arg ExprId, target, formal face) immutably, then mint one
/// thunk FnDecl per distinct (target, formal face) — in first-seen
/// order, so names are build-deterministic — and rewrite each
/// admitted argument Ident in place to the thunk's name.
pub fn synthesize_sig_thunks(ast: &mut Ast) {
    let faces = snapshot_faces(ast);
    if faces.is_empty() {
        return;
    }
    // Phase A — collect (arg eid, target name, formal face) sites.
    let mut sites: Vec<(ExprId, String, Vec<String>, String)> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(cname) = ast.get_expr(*callee) else {
            continue;
        };
        let Some(cface) = faces.get(cname) else {
            continue;
        };
        for (i, aid) in args.iter().enumerate() {
            let Expr::Ident(gname) = ast.get_expr(*aid) else {
                continue;
            };
            let Some(target) = faces.get(gname) else {
                continue;
            };
            let Some((formal_ps, formal_ret)) =
                formal_face(cface.param_anns.get(i).map(String::as_str))
            else {
                continue;
            };
            if thunk_plan(target, &formal_ps, &formal_ret) {
                sites.push((*aid, gname.clone(), formal_ps, formal_ret));
            }
        }
    }
    // Phase A' — fn-typed LET slots (423-03 ④: `const slot: () =>
    // void = gb` — the checker's let-position admit mirrors the
    // call-face one, so the lowering must reabstract the same way;
    // an un-thunked init stores the bare address and the slot's
    // narrower call_indirect reads garbage). The recursive spine
    // reaches body-local lets.
    collect_let_sites(ast, &ast.stmts, &faces, &mut sites);
    if sites.is_empty() {
        return;
    }
    // Phase B — mint thunks (dedup by target + formal face, named in
    // first-seen order) and rewrite the argument Idents.
    let mut minted: HashMap<(String, Vec<String>, String), String> = HashMap::new();
    let mut seq = 0usize;
    for (aid, gname, formal_ps, formal_ret) in sites {
        let key = (gname.clone(), formal_ps.clone(), formal_ret.clone());
        let thunk_name = match minted.get(&key) {
            Some(n) => n.clone(),
            None => {
                let n = format!("__sigthunk_{gname}_{seq}");
                seq += 1;
                mint_thunk(ast, &n, &gname, faces[&gname].param_anns.len(), &key);
                minted.insert(key, n.clone());
                n
            }
        };
        ast.exprs[aid.0 as usize] = Expr::Ident(thunk_name);
    }
}

/// The Phase A' walk: every fn-type-annotated `LetDecl` whose init
/// is an admitted mismatched head-less FnDecl Ident, at any nesting
/// depth (the `collect_decls_by_name` recursion shape over the
/// shared nested-statement spine).
fn collect_let_sites(
    ast: &Ast,
    stmts: &[Stmt],
    faces: &HashMap<String, FnFace>,
    out: &mut Vec<(ExprId, String, Vec<String>, String)>,
) {
    for s in stmts {
        if let Stmt::LetDecl {
            type_ann: Some(ann),
            init,
            ..
        } = s
            && let Expr::Ident(gname) = ast.get_expr(*init)
            && let Some(target) = faces.get(gname)
            && let Some((formal_ps, formal_ret)) = formal_face(Some(ann))
            && thunk_plan(target, &formal_ps, &formal_ret)
        {
            out.push((*init, gname.clone(), formal_ps, formal_ret));
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_let_sites(ast, inner, faces, out)
        });
    }
}

/// Build one thunk FnDecl: head-less, params spelled exactly as the
/// formal face, body a single direct call of the target — formal
/// positions forwarded, declared-past-formal positions fed an
/// explicit `undefined` (the §10.2.1.4 binding the target's
/// materialized guards observe). A void/undefined formal return
/// discards the call value; anything else returns it.
fn mint_thunk(
    ast: &mut Ast,
    thunk_name: &str,
    target: &str,
    target_arity: usize,
    (_, formal_ps, formal_ret): &(String, Vec<String>, String),
) {
    let params: Vec<Param> = formal_ps
        .iter()
        .enumerate()
        .map(|(i, f)| Param {
            name: format!("__st_p{i}"),
            type_ann: Some(f.clone()),
            default: None,
            is_rest: false,
        })
        .collect();
    let mut arg_eids: Vec<ExprId> = Vec::with_capacity(target_arity);
    for i in 0..target_arity {
        let e = if i < formal_ps.len() {
            Expr::Ident(format!("__st_p{i}"))
        } else {
            Expr::Ident("undefined".into())
        };
        arg_eids.push(ast.add_expr(e));
    }
    let callee = ast.add_expr(Expr::Ident(target.to_string()));
    let call = ast.add_expr(Expr::Call {
        callee,
        args: arg_eids,
    });
    let body = if formal_ret == "void" || formal_ret == "undefined" {
        vec![Stmt::Expr(call)]
    } else {
        vec![Stmt::Return(Some(call))]
    };
    ast.stmts.push(Stmt::FnDecl {
        name: thunk_name.to_string(),
        type_params: Vec::new(),
        params,
        return_type: Some(formal_ret.clone()),
        body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
}
