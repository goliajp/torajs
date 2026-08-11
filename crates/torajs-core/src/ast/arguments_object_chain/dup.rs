//! Rotation 362 — the dup-named binding group admit, split from
//! `arguments_object_chain.rs` (file-size line). See the fn doc; the
//! parent keeps the solo-chain walk and the shared legal-use set.

use super::super::{Ast, Expr, ExprId, Stmt};
use super::{collect_legal_uses, collect_letdecls_recursive};

/// Rotation 362 — per-scope admit for a DUP-named binding group (two
/// fns each declaring `const f = function () { …arguments… }`, the
/// test262 helper idiom). The by-name kill walk cannot split scopes,
/// so the group is admitted only as a WHOLE, under conditions that
/// make the shared name unambiguous:
/// * every instance sits in a distinct top-level FnDecl body (a
///   module-top or same-owner instance rejects the group);
/// * every instance's init is a closure literal, at least one of
///   which passes this tier's seed (rotation 364 — a mixed-tier
///   group returns only its seeded instances; see the body comment
///   for why the cross-tier name collision is safe);
/// * every arena `Ident(name)` lands inside exactly one owner's
///   expression mask (no stray module-top use);
/// * per instance (seeded or not), the mask-local uses pass the same
///   legal-use / boxed-consumption test as a solo chain (alias
///   growth is NOT offered — a nested `const g = f` kills the
///   group).
pub(super) fn admit_dup_groups(
    ast: &Ast,
    dup: &std::collections::HashSet<&str>,
    seed: &impl Fn(&str) -> bool,
    assign_legal: &std::collections::HashSet<u32>,
    boxed_face_store: &impl Fn(ExprId) -> bool,
    boxed_arg_sites: &std::collections::HashSet<ExprId>,
) -> Vec<(String, String)> {
    use std::collections::{HashMap, HashSet};
    if dup.is_empty() {
        return Vec::new();
    }
    // Owner-tagged lets: instances inside a top-level FnDecl body get
    // that FnDecl's stmt index; everything else (module top, and lets
    // inside fns nested under non-FnDecl top-level containers) gets
    // None, which rejects its group below — conservative.
    let mut owned_lets: Vec<(Option<usize>, &str, ExprId)> = Vec::new();
    for (si, s) in ast.stmts.iter().enumerate() {
        let mut lets: Vec<(&str, ExprId)> = Vec::new();
        let owner = if let Stmt::FnDecl { body, .. } = s {
            collect_letdecls_recursive(body, &mut lets);
            Some(si)
        } else {
            collect_letdecls_recursive(core::slice::from_ref(s), &mut lets);
            None
        };
        owned_lets.extend(lets.into_iter().map(|(n, i)| (owner, n, i)));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for name in dup {
        let insts: Vec<(Option<usize>, ExprId)> = owned_lets
            .iter()
            .filter(|(_, n, _)| n == name)
            .map(|(o, _, i)| (*o, *i))
            .collect();
        let owners: HashSet<Option<usize>> = insts.iter().map(|(o, _)| *o).collect();
        if owners.contains(&None) || owners.len() != insts.len() {
            continue;
        }
        // Rotation 364 — mixed-tier groups admit per instance: every
        // init must still be a closure literal (a non-closure init
        // leaves the shared name's meaning open — kill the group),
        // but only the instances passing THIS tier's seed are
        // returned. The other tier's instances get the same
        // treatment from its own admit call; the resulting name
        // collision across the two locals sets is safe — the
        // checker's argv wedge re-gates on the fn's public variadic
        // type, S3.5 admits beyond-arity universally, the SSA
        // argv_face mis-hit only downgrades to the boxed dual entry
        // (behaviorally equivalent), and fnexpr_this_routed's
        // consumption is a conservative exclusion arm. Full audit:
        // tasks/2026-08-11/dup-group-mixed-tier-design.md.
        let fns: Vec<(&str, bool)> = insts
            .iter()
            .filter_map(|(_, init)| match ast.get_expr(*init) {
                Expr::Closure { fn_name, .. } => Some((fn_name.as_str(), seed(fn_name))),
                _ => None,
            })
            .collect();
        if fns.len() != insts.len() || !fns.iter().any(|(_, s)| *s) {
            continue;
        }
        let masks: Vec<Vec<bool>> = insts
            .iter()
            .map(|(o, _)| {
                let mut m = vec![false; ast.exprs.len()];
                if let Some(Stmt::FnDecl { body, .. }) = ast.stmts.get(o.unwrap()) {
                    super::super::desugar_eval::body_owned_exprs(ast, body, &mut m);
                }
                m
            })
            .collect();
        let in_any_mask =
            |i: usize| -> bool { masks.iter().any(|m| m.get(i).copied().unwrap_or(false)) };
        let stray_use = ast.exprs.iter().enumerate().any(|(i, e)| {
            (matches!(e, Expr::Ident(n) if n == name)
                || matches!(e, Expr::Closure { captures, .. }
                    if captures.iter().any(|c| c == *name)))
                && !in_any_mask(i)
        });
        if stray_use {
            continue;
        }
        let empty: HashMap<String, String> = HashMap::new();
        let all_pass = masks.iter().all(|mask| {
            // A capture of the name inside this owner's mask is a
            // reference the walk can't see through — reject.
            if ast.exprs.iter().enumerate().any(|(i, e)| {
                mask[i]
                    && matches!(e, Expr::Closure { captures, .. }
                        if captures.iter().any(|c| c == *name))
            }) {
                return false;
            }
            let b_eids: HashSet<u32> = (0..ast.exprs.len() as u32)
                .filter(|&i| {
                    mask[i as usize]
                        && matches!(&ast.exprs[i as usize], Expr::Ident(n) if n == name)
                })
                .collect();
            let legal = collect_legal_uses(ast, &empty, &b_eids, assign_legal, boxed_face_store);
            !b_eids
                .difference(&legal)
                .any(|&i| !boxed_arg_sites.contains(&ExprId(i)))
        });
        if all_pass {
            for (fn_name, seeded) in &fns {
                if *seeded {
                    out.push((name.to_string(), fn_name.to_string()));
                }
            }
        }
    }
    out
}
