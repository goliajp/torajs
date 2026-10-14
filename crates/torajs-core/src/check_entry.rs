//! Public entry-point cluster for the type-checker — pulled out of
//! [`crate::check`] as chunk-318 of the check.rs god-file decomp.
//!
//! Six thin wrappers that all share the same shape:
//!   1. construct a `Checker`,
//!   2. run the full pipeline,
//!   3. filter / project the result into the per-caller return shape.
//!
//! Kept in their own file because callers (CLI `tr run` / `tr build`,
//! LSP `collect_diagnostics`, codegen-corpus tooling) reach them via
//! `torajs_core::check::<name>` — the re-export in `check.rs` preserves
//! that path.
//!
//! See `CheckArtifacts` for the full T-28 artifact tuple.
//!
//! Visibility: all six fns + `CheckArtifacts` are `pub` (same as before
//! the split). `Checker::new` and `Checker::run_full_pipeline` were
//! promoted from `fn` to `pub(crate) fn` to let this sibling module
//! drive them.

use crate::ast::{Ast, ExprId};
use crate::check::{Diagnostic, GenericCallSites, Severity, Type};
use std::collections::{HashMap, HashSet};

mod fixpoint;
mod hidden_any;
pub(crate) mod shadowed_reads;

use fixpoint::{check_to_fixpoint, fixpoint_hidden};

pub fn check(ast: &Ast) -> Result<GenericCallSites, String> {
    check_with_types(ast).map(|(g, _)| g)
}

/// T-15.g.6 (v0.5.0) — typed variant of `check`. Also returns the
/// per-Expr type map so ssa_lower can recover Promise<T>'s inner T
/// at the await Member-access site (Type::Promise is unit at SSA;
/// PromiseId interning would be the cleaner fix but threads through
/// 22+ parse_type call sites — this side-channel is the smaller
/// change).
pub fn check_with_types(ast: &Ast) -> Result<(GenericCallSites, HashMap<ExprId, Type>), String> {
    check_with_arity(ast).map(|(g, t, _, _, _, _, _)| (g, t))
}

/// T-28 — full pipeline artifacts: generic call sites, per-Expr types,
/// per-Call arity pad map, the demoted speculative-rewrite map
/// (name-based class-method rewrites whose receiver checked as a
/// builtin container; the member-call shape is restored inside the
/// mono pass — see cm_demote.rs), the contextual-any literal set, and
/// the monomorphization output (owned post-restore AST with
/// specialization FnDecls appended + call retargets + generic names
/// for lower pass-1 to skip — see check_monomorph.rs).
pub type CheckArtifacts = (
    GenericCallSites,
    HashMap<ExprId, Type>,
    HashMap<ExprId, usize>,
    HashMap<ExprId, ExprId>,
    std::collections::HashSet<ExprId>,
    crate::check_monomorph::MonoOutput,
    HashMap<String, String>,
);

/// T-28 — check that also returns the per-Call arity pad + demotion
/// maps. New callers (main.rs `tr run` / `tr build`) use this; the
/// older `check_with_types` is kept for back-compat (tests, lsp).
///
/// Runs post-check monomorphization (specializations emitted and
/// checked inference-only — RFC 20260713-mono-check-specializations)
/// after the main pipeline. A specialization's diagnostics are
/// discarded inside the pass (see check_monomorph.rs) unless the body
/// left a generic call with no retarget — that one rejects, because
/// the alternative is the lowerer panicking on the callee's name.
pub fn check_with_arity(ast: &Ast) -> Result<CheckArtifacts, String> {
    check_with_arity_warn(ast).map(|(artifacts, _)| artifacts)
}

/// RFC 20260730-undeclared-ident — variant that also surfaces
/// warnings. `tr run` / `tr build` print them to stderr without
/// rejecting — the tsc posture (diagnose + still emit). Undeclared-
/// read warnings synthesize here from the surviving mark set (one
/// per name, sorted — marks self-heal during the pipeline, so only
/// end-state marks warn); any Warning-severity diagnostics ride
/// along. Collected before the mono pass, which discards its own
/// diagnostics except where a specialization body left a hole.
///
/// A round can ask for bindings to be retyped `any`
/// ([`hidden_any::settle`]).
pub fn check_with_arity_warn(ast: &Ast) -> Result<(CheckArtifacts, Vec<String>), String> {
    crate::iface_witness::begin_compile();
    let (artifacts, warnings, census) =
        hidden_any::settle(ast, check_round(ast, &HashSet::new())?)?;
    if let Some(line) = census {
        eprintln!("{line}");
    }
    Ok((artifacts, warnings))
}

/// The artifacts, the warnings and the typed census line of a round;
/// the line comes back unprinted unless the round failed.
type Round = (CheckArtifacts, Vec<String>, Option<String>);

/// One check of `ast` and its mono pass, reading the call arguments
/// `hidden` holds as values of any kind.
fn check_round(ast: &Ast, hidden: &HashSet<ExprId>) -> Result<Round, String> {
    let mut c = fixpoint_hidden(ast, hidden);
    // typed 覆盖率读数(`TORAJS_TYPED_CENSUS=1` 下才打)。在错误分支
    // 提前返回**之前**算 —— 一次被拒的编译同样要有读数,否则 `refused`
    // 永远是 0。见 `crate::typed_census`。
    let census = c.typed_census_line(ast);
    let error_messages: Vec<String> = c
        .errors
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    // S0 的实测闸(`crate::s0_admit`):开着时这些诊断降级成 warning,
    // 让被拒的程序照常 lower —— 于是 test262 那 366 条被拒的 case 自己
    // 就是判据 2 的探针。闸关着时这一行的唯一效果是读一次环境变量。
    let admitted = if crate::s0_admit::admit() {
        error_messages
    } else {
        if !error_messages.is_empty() {
            if let Some(line) = &census {
                eprintln!("{line}");
            }
            return Err(error_messages.join("\n"));
        }
        Vec::new()
    };
    // A `with` fall-through read is not a diagnostic: see
    // `Ast::with_fallthrough_idents`. The entry stays in
    // `undeclared_reads` so lowering still emits the ReferenceError
    // path for the case where the object does not carry the name.
    let mut undeclared: Vec<&String> = c
        .undeclared_reads
        .iter()
        .filter(|(eid, _)| !ast.with_fallthrough_idents.contains(eid))
        .map(|(_, n)| n)
        .collect();
    undeclared.sort();
    undeclared.dedup();
    let mut warnings: Vec<String> = undeclared
        .into_iter()
        .map(|n| format!("unknown identifier `{n}`"))
        .collect();
    warnings.extend(
        c.errors
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .map(|d| d.message.clone()),
    );
    // 闸开时把降级掉的诊断带出来,否则一次 admit 过的编译与一次本来就
    // 干净的编译在调用方看来一模一样 —— 探针要分得清这两者。
    warnings.extend(admitted.into_iter().map(|m| format!("[s0-admit] {m}")));
    let before_mono = c.errors.len();
    let mut mono = crate::check_monomorph::monomorphize_and_check(&mut c, ast);
    mono.mono_ast.face_slots = std::mem::take(&mut c.face_slots);
    mono.mono_ast.face_slots.ret_any = std::mem::take(&mut c.fn_slot_ret_any);
    // A specialization body's diagnostics are discarded inside the
    // pass — except when the body left a HOLE (a generic call with no
    // retarget). Those are not diagnostics about a program that
    // lowers; they say it cannot, and the lowerer's own answer to
    // them is a panic naming the callee. See
    // `check_monomorph_uninferred`.
    let mono_errors: Vec<String> = c.errors[before_mono..]
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    if !mono_errors.is_empty() {
        if let Some(line) = &census {
            eprintln!("{line}");
        }
        return Err(mono_errors.join("\n"));
    }
    // a round the driver compiles again for the generator twins it
    // asked for is not the program's reading
    let census = census.filter(|_| {
        mono.mono_ast
            .generator_twin_requests
            .is_subset(&ast.generator_twin_requests)
    });
    Ok((
        (
            c.generic_call_sites,
            c.expr_types,
            c.arity_pad_count,
            c.demoted_cm_rewrites,
            c.contextual_any_literals,
            mono,
            c.global_spellings,
        ),
        warnings,
        census,
    ))
}

/// v0.3 #5 LSP — string-typed errors-only collector kept for back-
/// compat. Filters out warnings so callers that historically only
/// surfaced errors keep the same shape. New callers should prefer
/// `collect_diagnostics` (T-04).
pub fn collect_errors(ast: &Ast) -> Vec<String> {
    let c = check_to_fixpoint(ast);
    c.errors
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect()
}

/// T-04 (v0.3.0) — full diagnostic stream with source spans + severity.
/// LSP consumes this to publish per-site squiggles + warning bucket
/// (lint also reads this same stream once it lands in T-06).
pub fn collect_diagnostics(ast: &Ast) -> Vec<Diagnostic> {
    let c = check_to_fixpoint(ast);
    c.errors
}

/// v0.3 #5 LSP L-3 — run the full typecheck pipeline and return
/// the per-Expr type table (populated as a side-effect by every
/// `type_of` call). Caller looks up by ExprId. Errors during
/// typecheck don't abort the table — partial coverage on the
/// reachable Exprs is still useful for hover. Errors are stringified
/// (errors-only, no warnings) for back-compat with existing LSP
/// hover code; full diagnostics flow through `collect_diagnostics`.
pub fn collect_types_and_errors(ast: &Ast) -> (HashMap<ExprId, Type>, Vec<String>) {
    let c = check_to_fixpoint(ast);
    let errs: Vec<String> = c
        .errors
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect();
    (c.expr_types, errs)
}
