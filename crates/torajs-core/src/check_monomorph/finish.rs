//! The tail of the monomorphization pass: drain the any-widen
//! fixpoint, hand the checker's side tables over to the AST the
//! lowerer reads, and build the num-width table over that final AST.
//!
//! The width table is built here rather than at the start of lowering
//! so the check pipeline can read its escape facts: a typed array
//! whose alias class escapes into the `any` world lowers as an
//! `Arr<Any>`, and its callbacks need what the checker mints for
//! values called through `any`. Lowering reads the same table, built
//! once from the same inputs.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::Checker;
use crate::check_monomorph::{MonoOutput, WorkItem};
use crate::check_monomorph_any_widen::struct_escape::escaped_cb;
use crate::num_width::WidthTable;
use crate::ssa_lower_generics_monomorph::Generics;

/// Run the any-widen fixpoint to completion and assemble the
/// [`MonoOutput`] the lowerer consumes. The width table is built over
/// the final AST; when it shows callbacks of escaped arrays without a
/// twin (`struct_escape::escaped_cb`), those are minted, the fixpoint
/// drains again, and the table is rebuilt over the grown AST.
#[allow(clippy::too_many_arguments)]
pub(crate) fn finish(
    c: &mut Checker,
    mut owned_ast: Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    mut call_retargets: HashMap<ExprId, String>,
    mut mono_decls: Vec<Stmt>,
    mut generic_fn_names: HashSet<String>,
) -> MonoOutput {
    let mut tried_shells = HashSet::new();
    loop {
        // retarget every admitted `Array(Any)`-into-`Array(T)` call site to
        // an any-widened clone; alternates with the generic worklist
        crate::check_monomorph_any_widen::run(
            c,
            &mut owned_ast,
            generics,
            cache,
            worklist,
            &mut call_retargets,
            &mut mono_decls,
            &mut generic_fn_names,
        );
        let appended = mono_decls.len();
        owned_ast.stmts.append(&mut mono_decls);
        let widths = hand_over(c, &mut owned_ast, &call_retargets);
        let minted = escaped_cb::mint_escaped_callback_twins(
            c,
            &mut owned_ast,
            generics,
            cache,
            worklist,
            &mut call_retargets,
            &mut mono_decls,
            &generic_fn_names,
            &mut tried_shells,
            &widths,
        );
        if !minted {
            crate::check_monomorph_any_widen::struct_escape::refuse_untwinned(
                c,
                &owned_ast,
                &generic_fn_names,
            );
            return MonoOutput {
                mono_ast: owned_ast,
                call_retargets,
                generic_fn_names,
                widths,
            };
        }
        mono_decls = hold_apart(&mut owned_ast, appended, mono_decls);
    }
}

/// The fixpoint runs on with the `appended` earlier specializations
/// held apart again, the new twins `minted` behind them.
fn hold_apart(owned_ast: &mut Ast, appended: usize, mut minted: Vec<Stmt>) -> Vec<Stmt> {
    let at = owned_ast.stmts.len() - appended;
    let mut held = owned_ast.stmts.split_off(at);
    held.append(&mut minted);
    held
}

/// The fields an object literal leaves out of the struct type the
/// checker recorded for it (`check_assignable::in_declared_order`).
fn absent_field_names(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
) -> std::collections::HashSet<crate::ast::PropKey> {
    let mut out = std::collections::HashSet::new();
    for (eid, ty) in expr_types {
        if let crate::ast::Expr::ObjectLit { fields } = ast.get_expr(*eid) {
            let absent = crate::check_assignable::literal_absent_fields(ty, fields);
            out.extend(absent.into_iter().map(|(_, n)| n));
        }
    }
    out
}

/// The checker's per-group lane verdicts (top-level and from the body
/// checks) ride to the lowerer on the AST it already reads; answers the
/// width table over that AST.
fn hand_over(
    c: &mut Checker,
    owned_ast: &mut Ast,
    call_retargets: &HashMap<ExprId, String>,
) -> WidthTable {
    owned_ast
        .iter_destr_srcs
        .extend(std::mem::take(&mut c.iter_destr_srcs));
    owned_ast
        .undeclared_reads
        .extend(std::mem::take(&mut c.undeclared_reads));
    owned_ast
        .self_name_writes
        .extend(std::mem::take(&mut c.self_name_writes));
    owned_ast
        .const_writes
        .extend(std::mem::take(&mut c.const_writes));
    owned_ast
        .spec_throws
        .extend(std::mem::take(&mut c.spec_throws));
    owned_ast.absent_field_names = absent_field_names(owned_ast, &c.expr_types);
    let origins = std::mem::take(&mut c.widen_clone_origin);
    owned_ast.widen_clone_origin.extend(origins);
    crate::check_monomorph_closures::prune_unresolved_captures(c, owned_ast);
    super::any_ret::retag(owned_ast, &c.expr_types);
    if let Err(e) = crate::iface_witness::settle(owned_ast) {
        crate::check::DiagPush::push_err(&mut c.errors, e);
    }
    // the width analysis below already asks which fields an accessor
    // can reach
    crate::ast::AccessorFieldNames::settle_key_reads(owned_ast, &c.expr_types);
    crate::num_width::analyze(
        owned_ast,
        call_retargets,
        &c.demoted_cm_rewrites,
        &c.expr_types,
    )
}
