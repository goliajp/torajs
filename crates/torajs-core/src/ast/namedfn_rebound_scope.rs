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
//! Two rebindings stay whole-name, because they are conflicts over the
//! declaration's own scope rather than a block carving a hole in it:
//! a parameter / catch / loop variable
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
pub(crate) fn rebound_beyond_blocks(stmts: &[Stmt], name: &str) -> bool {
    super::fnexpr_this_names::name_shadowed_elsewhere(stmts, name)
        || stmts
            .iter()
            .any(|s| matches!(s, Stmt::LetDecl { name: dn, .. } if dn == name))
}

/// Every arena slot reachable from a nested statement list that
/// declares `name`.
pub(crate) fn block_scoped_expr_ids(ast: &Ast, name: &str) -> HashSet<ExprId> {
    let mut out = HashSet::new();
    for s in &ast.stmts {
        walk_stmt(ast, s, name, false, &mut out);
    }
    out
}

/// `inside` is true once an enclosing list has declared `name`; from
/// there every slot below belongs to that binding.
fn walk_stmt(ast: &Ast, s: &Stmt, name: &str, inside: bool, out: &mut HashSet<ExprId>) {
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

fn declares(stmts: &[Stmt], name: &str) -> bool {
    stmts
        .iter()
        .any(|s| matches!(s, Stmt::LetDecl { name: dn, .. } if dn == name))
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
