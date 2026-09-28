//! Which arena slots a BLOCK-scoped rebinding of a fn name owns.
//!
//! `synthesize_fn_to_closure_forwarders` and `promote_this_fn_values`
//! both match a bare `Ident` against a table of `bind_this_param`-
//! promoted declarations over a FLAT arena carrying no scope
//! information, so neither can tell a read of the declaration from a
//! read of an inner binding that shadows it. Wrapping the latter turns
//! a program that answers `3` into one that answers `[Function: fn]`.
//!
//! Rotation 595 answered that with a whole-NAME refusal. It is safe —
//! the wrong value is never minted — but it pays in the other
//! direction: ONE block anywhere in the program rebinding the name
//! sends every OTHER value use of that declaration back to the answer
//! it gave before the promote pass existed. The first cut of that
//! refusal was wider still and test262 charged six pass regressions
//! for it; the narrowing to `this_param_fns` bought those back and
//! left this residue.
//!
//! The statement tree carries what the arena lost. This module reads
//! it back out: the slots inside a nested statement list that declares
//! `name` are exactly the reads that block owns, and refusing THOSE
//! leaves every other site rewritten.
//!
//! A top-level function's parameter carves out that function the same
//! way (rotation 662: a harness helper's `thunk` parameter refused every
//! value use of a user `function thunk`). Two rebindings stay
//! whole-name, because they are conflicts over the declaration's own
//! scope rather than a scope carving a hole in it: a nested function's
//! parameter / catch / loop variable
//! ([`super::fnexpr_this_names::name_shadowed_elsewhere`]), and a
//! redeclaration at the TOP level, which no block boundary separates
//! from the declaration.
//!
//! An arrow body is a statement list hanging off an EXPRESSION rather
//! than off a statement, so `for_each_nested_list` cannot reach it and
//! it is walked through [`walk::arrow_nodes`] — the same reason
//! `desugar_with` needed that helper.

use super::desugar_with::walk;
use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashSet;

/// Whether `name` is rebound in a way this module can NOT carve out
/// per site — the callers keep refusing such a name outright.
///
/// Three of them. A parameter / catch / loop variable and a TOP-level
/// redeclaration are conflicts over the declaration's own scope. A
/// `var` is the third and the least obvious: it is written inside a
/// block but BINDS at function scope, so the reads it owns are not
/// the reads under that block — carving per site would refuse the
/// wrong half.
pub(crate) fn rebound_beyond_blocks(stmts: &[Stmt], name: &str) -> bool {
    shadowed_outside_top_fn_params(stmts, name)
        || stmts
            .iter()
            .any(|s| matches!(s, Stmt::LetDecl { name: dn, .. } if dn == name))
        || var_anywhere(stmts, name)
}

/// [`super::fnexpr_this_names::name_shadowed_elsewhere`], except that a
/// top-level function's own parameter is a scope this module carves
/// out per site ([`block_scoped_expr_ids`] owns that function's body):
/// a parameter spelled like a declaration elsewhere in the program
/// shadows it inside that one function only. A catch / loop variable,
/// and a parameter of a nested function, stay whole-name.
fn shadowed_outside_top_fn_params(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::FnDecl { params, body, .. } if params.iter().any(|p| p.name == name) => {
            super::fnexpr_this_names::name_shadowed_elsewhere(body, name)
        }
        _ => super::fnexpr_this_names::name_shadowed_elsewhere(std::slice::from_ref(s), name),
    })
}

fn var_anywhere(stmts: &[Stmt], name: &str) -> bool {
    for s in stmts {
        if matches!(s, Stmt::LetDecl { name: dn, is_var: true, .. } if dn == name) {
            return true;
        }
        let mut hit = false;
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            hit = hit || var_anywhere(inner, name);
        });
        if hit {
            return true;
        }
    }
    false
}

/// Every arena slot reachable from a nested statement list that
/// declares `name`.
pub(crate) fn block_scoped_expr_ids(ast: &Ast, name: &str) -> HashSet<ExprId> {
    let mut out = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, .. } = s
            && params.iter().any(|p| p.name == name)
        {
            for d in params.iter().filter_map(|p| p.default) {
                collect_expr(ast, d, &mut out);
            }
            walk_stmt(ast, s, name, true, &mut out);
            continue;
        }
        walk_stmt(ast, s, name, false, &mut out);
    }
    out
}

/// `inside` is true once an enclosing list has declared `name`; from
/// there every slot below belongs to that binding.
fn walk_stmt(ast: &Ast, s: &Stmt, name: &str, inside: bool, out: &mut HashSet<ExprId>) {
    // Two statements hand `for_each_nested_list` several lists that
    // share ONE scope, so asking each list on its own under-refuses:
    // a `for` head's binding scopes over the head and the body, and a
    // switch's CaseBlock is one lexical scope across every case. Both
    // answer for the whole statement instead.
    //
    // Only the switch was measured to manifest — `case 1: const t = 3`
    // read from `case 2` answered `function` where bun answers
    // `number`. The `for` head is defensive: its own A/B answered the
    // same before and after, and nothing found a spelling that shows
    // it.
    let inside = inside || stmt_scope_declares(s, name);
    if inside {
        for e in walk::stmt_exprs(s) {
            collect_expr(ast, e, out);
        }
    }
    for a in walk::arrow_nodes(ast, s) {
        let Expr::ArrowFn { params, body, .. } = ast.get_expr(a) else {
            continue;
        };
        let now = inside || declares(body, name) || params.iter().any(|p| p.name == name);
        for c in body {
            walk_stmt(ast, c, name, now, out);
        }
    }
    super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
        let now = inside || declares(inner, name);
        for c in inner {
            walk_stmt(ast, c, name, now, out);
        }
    });
}

/// A statement whose own several lists share one scope, declaring
/// `name` in any of them.
fn stmt_scope_declares(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::For { init: Some(i), .. } => declares(std::slice::from_ref(&**i), name),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| declares(&c.body, name))
                || default.as_ref().is_some_and(|d| declares(d, name))
        }
        _ => false,
    }
}

/// A lexical declaration of `name` directly in this list. A `var` is
/// not one — [`rebound_beyond_blocks`] refuses those whole-name,
/// because the reads it owns reach outside the block it is written in.
fn declares(stmts: &[Stmt], name: &str) -> bool {
    super::stmts_flat(stmts)
        .into_iter()
        .any(|s| matches!(s, Stmt::LetDecl { name: dn, is_var: false, .. } if dn == name))
}

fn collect_expr(ast: &Ast, e: ExprId, out: &mut HashSet<ExprId>) {
    if !out.insert(e) {
        return;
    }
    for c in walk::expr_children(ast, e) {
        collect_expr(ast, c, out);
    }
}

/// Drop a `bind_this_param`-promoted candidate whose name some inner
/// scope rebinds — the same verdict on the same evidence the sibling
/// `namedfn_this_value` takes, through the same helper rather than a
/// second copy of it.
///
/// Every axis in the caller matches a bare Ident against this table
/// over a flat arena carrying no scope information, so none of them can tell a read of the declaration from
/// a read of an inner binding that shadows it. Wrapping the latter
/// turns a program that answers `3` into one that answers
/// `[Function: fn]` — a wrong value, not a wrong receiver.
///
/// The repro was carried, in words, by the whole-name helper itself:
/// rotation 594 took the verdict for the value-promote pass and missed
/// that THIS pass mints the same `__forward_` cell at the same
/// positions. The bug therefore survived in bare blocks, `if` bodies
/// and loop bodies while a function body answered right.
///
/// The promoted gate is the SYMPTOM's own boundary, measured both ways:
/// a declaration that never says `this` reads its inner shadow
/// correctly today, because the value it would be wrapped into is the
/// same cell either way. Refusing those too costs real programs — the
/// first cut did, and test262 answered with six pass regressions
/// (`Object/S15.2.1.1_A2_T9`, `S15.2.2.1_A2_T6`, two
/// `tagged-template/cache-*-new-function`, `statements/function/name`,
/// `sm/class/newTargetDVG`), all of them turning into
/// "not yet supported" because a downstream slot still wanted the
/// closure repr this pass had stopped minting.
///
/// What stays whole-name is now only what no block boundary separates
/// from the declaration — a parameter / catch / loop variable, and a
/// top-level redeclaration. A BLOCK rebinding hands back the arena
/// slots it owns instead ([`block_scoped_expr_ids`]), so the reads
/// outside it keep the forwarder they need: refusing the name outright
/// sent every other value use in the program back to the pre-promote
/// answer because one block somewhere reused the spelling.
pub(crate) fn refuse_rebound_names(
    ast: &Ast,
    fn_sigs: &mut std::collections::HashMap<
        String,
        (Vec<Param>, Option<String>, crate::lexer::Span),
    >,
) -> std::collections::HashSet<ExprId> {
    let mut refused = std::collections::HashSet::new();
    fn_sigs.retain(|n, _| {
        if !ast.this_param_fns.contains(n) {
            return true;
        }
        if rebound_beyond_blocks(&ast.stmts, n) {
            return false;
        }
        refused.extend(block_scoped_expr_ids(ast, n));
        true
    });
    refused
}
