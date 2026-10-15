//! 397-02 — the ALIAS-INIT closure: `const B = K` admits as a use of
//! `K` when every use of `B` is itself receiver-safe.
//!
//! An alias initializer was the one shape the receiver-safe list
//! could never take: after promotion the closure's ABI carries a
//! leading `__this` param, and a call through the alias would not
//! know it — the direct-call `undefined` seeding is keyed by the
//! PROMOTED name, so `B()` would eat the first argument. What makes
//! an alias admissible is therefore a property of the alias's OWN
//! uses, and the proof is transitive: `const C = B` chains, so the
//! set computes as a greatest fixpoint (the `safe_generic_param_names`
//! posture — start with every candidate in, refute until stable).
//!
//! Per-use rules inside the fixpoint:
//!
//! * every [`UseShapes`] shape admits — the value shapes never call,
//!   and the any-escaping shapes shift argv on FLAG_CLOSURE_RECV_FIRST
//!   wherever the value lands. The direct-call callee is name-keyed at
//!   the call site, so it admits only for an alias the promotion can
//!   put on that key alongside its source: declared without an
//!   annotation and never written again. [`called_alias_names`] hands
//!   such aliases to the promotion, which registers them next to the
//!   source or, where the source itself stays off the static list,
//!   declines;
//! * the argument of `Object.__forinKeys(v)` admits — §14.7.5.9
//!   EnumerateObjectProperties never calls the enumerated object,
//!   and this synthesized call is how the for-in head desugar
//!   (`make_forin_src`) consumes its hoisted snapshot binding. This
//!   is the shape that motivated the closure: `for (const k in K)`
//!   aliases `K` into `__forin_obj_N`, whose only Ident use is this
//!   argument;
//! * the init site of another currently-safe alias admits (the
//!   chain rule above);
//! * the TARGET of the assignment that introduced an alias admits —
//!   see [`collect_alias_decls`], which is also where the `var`
//!   spelling of an alias enters the set.
//!
//! An alias name declared more than once or shadowed by a non-LetDecl
//! declarator refutes outright — the by-name use walk could not be
//! paired to the binding (the same bar every knife-2 census holds).
//! `.call` / `.apply` / `.bind` reads refute for free: `member_obj`
//! excludes those names, so such a use admits nothing.

use super::fnexpr_this_names::{collect_decls_by_name, name_shadowed_elsewhere, peel_as};
use super::fnexpr_this_shapes::UseShapes;
use super::{Expr, ExprId, Stmt};

/// Every LetDecl-init Ident ExprId whose alias binding proves safe.
/// The routed promotion's `admits` unions this in, so the SOURCE
/// name's use at the alias declaration stops refuting its promotion.
pub(super) fn safe_alias_init_sites(
    stmts: &[Stmt],
    exprs: &[Expr],
    base: &UseShapes,
) -> std::collections::HashSet<ExprId> {
    // The never-calls argument slot of the for-in keys synthesis.
    let forin_keys_args: std::collections::HashSet<ExprId> = exprs
        .iter()
        .filter_map(|e| {
            let Expr::Call { callee, args } = e else {
                return None;
            };
            let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
                return None;
            };
            if name != "__forinKeys" {
                return None;
            }
            let Expr::Ident(o) = &exprs[obj.0 as usize] else {
                return None;
            };
            (o == "Object").then(|| args.first().copied()).flatten()
        })
        .collect();
    // Candidate aliases: a LetDecl whose (as-peeled) init is a bare
    // Ident, or the statement-position `X = <name>` the `var` split
    // mints. The alias name must be pairable by name.
    let mut aliases: Vec<(ExprId, String)> = Vec::new();
    let mut assign_targets: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    collect_alias_decls(stmts, exprs, &mut aliases, &mut assign_targets);
    aliases.retain(|(_, b)| {
        let mut decls = Vec::new();
        collect_decls_by_name(stmts, b, &mut decls);
        decls.len() == 1 && !name_shadowed_elsewhere(stmts, b)
    });
    let callable = callable_aliases(stmts, exprs, base, &aliases, &assign_targets);
    let mut safe: std::collections::HashSet<String> =
        aliases.iter().map(|(_, b)| b.clone()).collect();
    loop {
        let safe_inits: std::collections::HashSet<ExprId> = aliases
            .iter()
            .filter(|(_, b)| safe.contains(b))
            .map(|(e, _)| *e)
            .collect();
        let mut changed = false;
        for (_, b) in &aliases {
            if !safe.contains(b) {
                continue;
            }
            let refuted = exprs.iter().enumerate().any(|(i, e)| {
                let Expr::Ident(n) = e else { return false };
                if n != b {
                    return false;
                }
                let eid = ExprId(i as u32);
                let ok = (base.admits(eid, b)
                    && (!base.callee.contains(&eid) || callable.contains(b.as_str())))
                    || forin_keys_args.contains(&eid)
                    || assign_targets.contains(&eid)
                    || safe_inits.contains(&eid);
                !ok
            });
            if refuted {
                safe.remove(b);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    aliases
        .into_iter()
        .filter(|(_, b)| safe.contains(b))
        .map(|(e, _)| e)
        .collect()
}

/// Both alias spellings, plus the target Ident nodes of the second one.
///
/// The second spelling is what a `var` alias looks like by the time
/// this pass runs: `desugar_var_hoist` splits `var X = K` into a
/// declaration without an initializer and a statement-position
/// assignment, so the LetDecl arm sees no init at all and the source
/// name's only remaining appearance is an assignment right-hand side —
/// a position nothing admits. Every `var X = K` therefore un-promoted
/// `K`, while the `let` / `const` spelling of the same three
/// characters promoted fine.
///
/// The proof is the one the LetDecl arm already carries: what makes an
/// alias admissible is a property of the alias's OWN uses, and those
/// are read by name either way. The target Ident is handed back so the
/// fixpoint does not read it as a use — a LetDecl carries its name as
/// a `String` field, but an assignment spells it as an arena node, and
/// left alone that node refutes every alias it introduces.
///
/// Statement position only. `f(X = K)` hands the assignment's VALUE on
/// to another position, which would need its own reading.
fn collect_alias_decls(
    stmts: &[Stmt],
    exprs: &[Expr],
    out: &mut Vec<(ExprId, String)>,
    targets: &mut std::collections::HashSet<ExprId>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                let init = peel_as(exprs, *init);
                if matches!(&exprs[init.0 as usize], Expr::Ident(_)) {
                    out.push((init, name.clone()));
                }
            }
            Stmt::Expr(eid) => {
                if let Expr::Assign { target, value } = &exprs[eid.0 as usize]
                    && let Expr::Ident(name) = &exprs[target.0 as usize]
                {
                    let value = peel_as(exprs, *value);
                    if matches!(&exprs[value.0 as usize], Expr::Ident(_)) {
                        out.push((value, name.clone()));
                        targets.insert(*target);
                    }
                }
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_alias_decls(inner, exprs, out, targets)
        });
    }
}

/// Aliases whose direct calls the promotion can seed: declared by an
/// unannotated `LetDecl` (the type follows the source, so the call
/// takes the same static lane) and never the target of a write other
/// than the one that introduced them.
fn callable_aliases<'a>(
    stmts: &[Stmt],
    exprs: &[Expr],
    base: &UseShapes,
    aliases: &'a [(ExprId, String)],
    intro_targets: &std::collections::HashSet<ExprId>,
) -> std::collections::HashSet<&'a str> {
    let mut unannotated = std::collections::HashSet::new();
    collect_unannotated_decl_names(stmts, &mut unannotated);
    aliases
        .iter()
        .map(|(_, b)| b.as_str())
        .filter(|b| unannotated.contains(*b) && !base.any_ann_names.contains(*b))
        .filter(|b| {
            !exprs.iter().enumerate().any(|(i, e)| {
                let eid = ExprId(i as u32);
                matches!(e, Expr::Ident(n) if n == b)
                    && base.assign_target.contains(&eid)
                    && !intro_targets.contains(&eid)
            })
        })
        .collect()
}

fn collect_unannotated_decl_names(stmts: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in stmts {
        if let Stmt::LetDecl {
            name,
            type_ann: None,
            ..
        } = s
        {
            out.insert(name.clone());
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_unannotated_decl_names(inner, out)
        });
    }
}

/// The safe aliases of `source`, directly or through other aliases,
/// that are called by name — the names the promotion of `source` must
/// put on the direct-call key with it.
pub(super) fn called_alias_names(
    stmts: &[Stmt],
    exprs: &[Expr],
    shapes: &UseShapes,
    source: &str,
) -> Vec<String> {
    let mut aliases: Vec<(ExprId, String)> = Vec::new();
    collect_alias_decls(stmts, exprs, &mut aliases, &mut Default::default());
    let mut reach: Vec<String> = vec![source.to_string()];
    let mut i = 0;
    while i < reach.len() {
        for (init, b) in &aliases {
            let from = matches!(&exprs[init.0 as usize], Expr::Ident(n) if *n == reach[i]);
            if from && shapes.safe_alias_init.contains(init) && !reach.contains(b) {
                reach.push(b.clone());
            }
        }
        i += 1;
    }
    reach.remove(0);
    reach.retain(|b| {
        exprs.iter().enumerate().any(|(i, e)| {
            matches!(e, Expr::Ident(n) if n == b) && shapes.callee.contains(&ExprId(i as u32))
        })
    });
    reach
}
