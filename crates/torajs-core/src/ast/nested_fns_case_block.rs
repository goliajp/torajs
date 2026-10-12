//! A `function` declared directly in a `switch` clause.
//!
//! ```text
//! switch (x) {
//!   case 0: function f() { return 7 }
//!           return 0;
//!   case 1: return f();     // node: 7
//! }
//! ```
//!
//! Every clause of a CaseBlock shares one declarative environment
//! (§14.12.4), and BlockDeclarationInstantiation initializes each
//! function declared anywhere in it before any clause runs. Lifting the
//! clauses one list at a time gave each its own scope, so a call from
//! another clause found nothing.
//!
//! The binding goes in front of the switch, in a block wrapped around
//! it, because a CaseBlock has no statement position of its own that
//! every entry point runs through. It takes a fresh name and the
//! references inside the case selectors and clause bodies follow it:
//! the scrutinee is evaluated outside the CaseBlock, so a same-named
//! binding there must keep its meaning, and in sloppy code the §B.3.3
//! write at the declaration's position addresses the var binding by the
//! source name.

use super::annexb_fn_var::{AnnexB, LiftCtx, LiftMode};
use super::desugar_generators_alpha::rebinds_in_stmt;
use super::nested_fns::collect_nested_fns_in_stmt;
use super::nested_fns_capture::is_synthetic;
use super::nested_fns_idents::{rewrite_idents_in_expr, rewrite_idents_in_stmt};
use super::stmt_nested_lists::for_each_nested_vec_mut;
use super::{Ast, Expr, ExprId, Stmt, SwitchCase};
use std::collections::{HashMap, HashSet};

/// What the rename half needs about one wrapped switch.
pub(super) struct CaseBlockSlots {
    /// `(source name, slot name)`; the first slot is the wrapper's first
    /// statement, which is how the rename half finds the switch again.
    slots: Vec<(String, String)>,
    /// §B.3.3 writes minted as `<name> = <slot>`. Their target is the
    /// var binding, so it keeps the source name through the rename.
    writes: Vec<(ExprId, String)>,
}

/// The clause bodies in source order — a `default` written between
/// cases sits between them, and the last of two same-named
/// declarations is the one the binding holds.
pub(super) fn clauses<'a>(
    cases: &'a mut [SwitchCase],
    default: &'a mut Option<Vec<Stmt>>,
    default_at: usize,
) -> Vec<&'a mut Vec<Stmt>> {
    let (before, after) = cases.split_at_mut(default_at);
    let mut out: Vec<&mut Vec<Stmt>> = before.iter_mut().map(|c| &mut c.body).collect();
    out.extend(default.as_mut());
    out.extend(after.iter_mut().map(|c| &mut c.body));
    out
}

/// Lift the declarations of the switch `stmt` with one binding per name
/// for the whole CaseBlock, then wrap the switch in the block that
/// holds those bindings.
pub(super) fn lift_case_block(
    stmt: &mut Stmt,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    branch_scoped: &mut HashSet<String>,
    mode: LiftMode,
    lifted: &mut Vec<Stmt>,
    ctx: &mut LiftCtx,
) {
    let Stmt::Switch {
        cases,
        default,
        default_at,
        ..
    } = stmt
    else {
        unreachable!("caller matched a switch");
    };
    let mode = mode.descend();
    let mut bodies = clauses(cases, default, *default_at);
    let mark = ctx.enter_binding_none();
    for b in bodies.iter() {
        ctx.enter_scope(&b[..]);
    }
    // (source name, slot, the mangled declaration it holds)
    let mut slots: Vec<(String, String, String)> = Vec::new();
    let mut writes: Vec<(ExprId, String)> = Vec::new();
    for body in bodies.iter_mut() {
        for s in body.iter_mut() {
            let (name, span) = match s {
                Stmt::FnDecl { name, span, .. } if !is_synthetic(name) => (name.clone(), *span),
                other => {
                    collect_nested_fns_in_stmt(
                        other,
                        parent_name,
                        renames,
                        branch_scoped,
                        mode,
                        lifted,
                        ctx,
                    );
                    continue;
                }
            };
            let mangled = format!("__nested_{parent_name}_{name}_{}", ctx.counter);
            ctx.counter += 1;
            ctx.source_names.push((mangled.clone(), name.clone()));
            renames.insert(name.clone(), mangled.clone());
            let slot = match slots.iter_mut().find(|(n, _, _)| *n == name) {
                Some(entry) => {
                    entry.2 = mangled.clone();
                    entry.1.clone()
                }
                None => {
                    let slot = format!("__swfn_{name}_{}", ctx.counter);
                    ctx.counter += 1;
                    slots.push((name.clone(), slot.clone(), mangled.clone()));
                    slot
                }
            };
            let verdict = ctx.annexb(&name, span, mode);
            if !matches!(verdict, AnnexB::Legacy) {
                branch_scoped.insert(name.clone());
            }
            let site = match verdict {
                AnnexB::Apply => {
                    let write = ctx.annexb_var_decl(&name, &slot);
                    if let Stmt::Expr(id) = &write {
                        writes.push((*id, name.clone()));
                    }
                    write
                }
                AnnexB::Skip | AnnexB::Strict | AnnexB::Legacy => Stmt::Block(Vec::new()),
            };
            let mut decl = std::mem::replace(s, site);
            if let Stmt::FnDecl { name: n, .. } = &mut decl {
                *n = mangled;
            }
            lifted.push(decl);
        }
    }
    ctx.leave_scope(mark);
    if slots.is_empty() {
        return;
    }
    let mut wrapper: Vec<Stmt> = slots
        .iter()
        .map(|(_, slot, mangled)| ctx.mint_block_decl(slot, mangled))
        .collect();
    ctx.case_blocks.push(CaseBlockSlots {
        slots: slots.into_iter().map(|(n, s, _)| (n, s)).collect(),
        writes,
    });
    wrapper.push(std::mem::replace(stmt, Stmt::Block(Vec::new())));
    *stmt = Stmt::Block(wrapper);
}

/// Point the references inside each wrapped CaseBlock at its slots.
/// Runs after the flush, because the writes it restores were minted
/// during the walk.
///
/// Inner switches first — the walk finishes an inner one before the
/// outer one, so that is the order `pending` is in. A reference an
/// inner CaseBlock already took is under a fresh name by the time the
/// outer one renames. A statement that binds the name again anywhere
/// inside it is left alone, as `switch_tdz_flag` does: its occurrences
/// may be the inner binding's, and leaving one can only miss.
pub(super) fn apply_case_block_renames(
    ast: &mut Ast,
    body: &mut [Stmt],
    pending: Vec<CaseBlockSlots>,
) {
    for cb in &pending {
        let mut visit = |v: &mut Vec<Stmt>| {
            if !matches!(v.first(), Some(Stmt::LetDecl { name, .. }) if *name == cb.slots[0].1) {
                return;
            }
            let Some(Stmt::Switch { cases, default, .. }) = v.last_mut() else {
                unreachable!("the wrapper ends in its switch");
            };
            rename_case_block(ast, cases, default, &cb.slots);
            for (id, name) in &cb.writes {
                if let Expr::Assign { target, .. } = ast.exprs[id.0 as usize] {
                    ast.exprs[target.0 as usize] = Expr::Ident(name.clone());
                }
            }
        };
        for s in body.iter_mut() {
            for_each_nested_vec_mut(s, &mut visit);
        }
    }
}

/// Rename each `(source name, slot)` in the case selectors and clause
/// bodies of one switch — not its scrutinee, which is evaluated outside
/// the CaseBlock.
pub(super) fn rename_case_block(
    ast: &mut Ast,
    cases: &mut [SwitchCase],
    default: &mut Option<Vec<Stmt>>,
    slots: &[(String, String)],
) {
    for (name, slot) in slots {
        let map: HashMap<String, String> = HashMap::from([(name.clone(), slot.clone())]);
        for c in cases.iter_mut() {
            rewrite_idents_in_expr(ast, c.value, &map, true);
        }
        let bodies = cases
            .iter_mut()
            .map(|c| &mut c.body)
            .chain(default.as_mut());
        for s in bodies.flat_map(|b| b.iter_mut()) {
            if !rebinds_in_stmt(s, name) {
                rewrite_idents_in_stmt(ast, s, &map, true);
            }
        }
    }
}
