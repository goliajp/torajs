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
//! * every [`UseShapes`] shape EXCEPT the direct-call callee admits —
//!   the value shapes never call, and the any-escaping shapes shift
//!   argv on FLAG_CLOSURE_RECV_FIRST wherever the value lands. The
//!   callee shape is exactly the one that is name-keyed at the call
//!   site, so it refutes;
//! * the argument of `Object.__forinKeys(v)` admits — §14.7.5.9
//!   EnumerateObjectProperties never calls the enumerated object,
//!   and this synthesized call is how the for-in head desugar
//!   (`make_forin_src`) consumes its hoisted snapshot binding. This
//!   is the shape that motivated the closure: `for (const k in K)`
//!   aliases `K` into `__forin_obj_N`, whose only Ident use is this
//!   argument;
//! * the init site of another currently-safe alias admits (the
//!   chain rule above).
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
    // Ident. The alias name must be pairable by name.
    let mut aliases: Vec<(ExprId, String)> = Vec::new();
    collect_alias_decls(stmts, exprs, &mut aliases);
    aliases.retain(|(_, b)| {
        let mut decls = Vec::new();
        collect_decls_by_name(stmts, b, &mut decls);
        decls.len() == 1 && !name_shadowed_elsewhere(stmts, b)
    });
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
                let ok = (base.admits(eid, b) && !base.callee.contains(&eid))
                    || forin_keys_args.contains(&eid)
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

fn collect_alias_decls(stmts: &[Stmt], exprs: &[Expr], out: &mut Vec<(ExprId, String)>) {
    for s in stmts {
        if let Stmt::LetDecl { name, init, .. } = s {
            let init = peel_as(exprs, *init);
            if matches!(&exprs[init.0 as usize], Expr::Ident(_)) {
                out.push((init, name.clone()));
            }
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_alias_decls(inner, exprs, out)
        });
    }
}
