//! One lib-section statement through the injection walk.
//!
//! [`super::resolve_imports`] answers "which modules load, and in what
//! order"; this file answers the narrower question it has to settle for
//! every statement it meets — does this statement inject, queue a
//! further load, or drop? Split out of `modules.rs` when the
//! `export * from` arm arrived.

use crate::ast::{Ast, Stmt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use super::inject_export::Landings;
use super::resolve_helpers::{queue_nested_import, queue_side_effect_reexport};
use super::static_resolution::RequestLedger;
use super::{
    NsAccum, WorkItem, decl_name, inject_bare_exported_decl, inject_export_inner, queue_reexport,
    queue_star_reexport,
};

/// Everything the walk needs about the request that reached this lib:
/// which names the importer asked for, how it wants them spelled, and
/// the namespace accumulator when the request was `import * as ns`.
pub(super) struct LibRequest<'a> {
    pub(super) side_effect_only: bool,
    pub(super) default_alias: &'a Option<String>,
    pub(super) want: &'a HashSet<&'a str>,
    pub(super) rename: &'a HashMap<&'a str, Vec<&'a str>>,
    pub(super) ns: Option<&'a mut NsAccum>,
    pub(super) bare_exports: &'a HashMap<String, String>,
    pub(super) own_exports: &'a HashSet<String>,
    /// The path's injected-name ledger (importer-visible names plus
    /// the request sentinels — see `resolve_imports`). The walk both
    /// reads it (a decl an earlier request injected must not inject
    /// again, whichever lane asks) and writes it (so a later NAMED
    /// request for a name this walk injected filters to nothing).
    pub(super) injected: &'a mut HashSet<String>,
    /// 423-01 deconflict — mangled local name → original export
    /// spelling, for the decls this walk's census renamed. The
    /// namespace accumulator claims FIELDS by the original spelling
    /// while referencing the mangled binding.
    pub(super) demangle: &'a HashMap<String, String>,
    /// 423-01 knife C — the hidden-dependency injection set: decls no
    /// request asked for but which an injected decl's free variables
    /// reach. Spellings are post-census (mangled, or the original
    /// name on a no-collision decline); they inject as plain
    /// top-level decls, never importer-visible.
    pub(super) hidden: &'a HashSet<String>,
    /// §16.2.1.6.3 ambiguity ledger — see [`Landings`].
    pub(super) landings: Landings<'a>,
    /// §16.2.1.6.4 steps 7 and 9 — this lib's own `import` clauses
    /// and indirect exports, judged after the BFS like the entry's
    /// (see [`RequestLedger`]).
    pub(super) ledger: RequestLedger<'a>,
    /// A dynamic-import candidate's walk collects its non-declaration
    /// top-level statements here instead of dropping them: the
    /// dispatcher runs them — once, in a job, an abrupt completion
    /// rejecting the promise — when the `import()` is evaluated
    /// (§16.2.1.5.3; `dyn_import_dispatcher::synth_eval_fn`). A
    /// static request's walk injects them in place instead (see
    /// `evaluate`).
    pub(super) deferred: Option<&'a mut Vec<Stmt>>,
    /// This walk runs the module body (see `LaneShape::evaluate`):
    /// its non-declaration statements inject in source order.
    pub(super) evaluate: bool,
}

/// One statement of a side-effect-only (`import "./x"`) lib walk —
/// the lib's ImportDecls still queue (so transitive loads happen),
/// but every other top-level statement (including bare expressions,
/// console.log, top-level `let`s, classes, and `export`-wrapped
/// decls) injects in source order. Bare named export (`export
/// { a }`, the P13-S4 re-export surface) is still rejected at the
/// side-effect boundary (K.2).
fn inject_side_effect_stmt(
    work: &mut VecDeque<WorkItem>,
    target_dir: &Path,
    injections: &mut Vec<Stmt>,
    s: Stmt,
    req: &mut LibRequest<'_>,
) -> Result<(), String> {
    if let Stmt::ImportDecl {
        source,
        named,
        default,
        namespace,
        ..
    } = s
    {
        req.ledger.import_clause(&source, &named, &default);
        queue_nested_import(work, target_dir, &source, named, default, namespace)?;
        return Ok(());
    }
    if let Stmt::ExportDecl {
        inner: None,
        star: Some(_),
        source: Some(star_source),
        ..
    } = &s
    {
        // A star re-export binds nothing, but the module it names
        // still has to be evaluated — §16.2.1.5 walks every requested
        // module of the module being evaluated, star clauses included.
        return queue_nested_import(work, target_dir, star_source, Vec::new(), None, None);
    }
    if let Stmt::ExportDecl {
        inner: None,
        star: None,
        source: Some(src),
        named,
        ..
    } = &s
    {
        // `export { … } from "m"` under a side-effect walk exposes no
        // face anyone can see, but m is a requested module
        // (§16.2.2.5) that evaluates, and every name the clause
        // forwards still has to resolve (§16.2.1.6.4 step 9).
        return queue_side_effect_reexport(work, target_dir, named, src, &mut req.ledger);
    }
    let stmt = if let Stmt::ExportDecl {
        inner: Some(boxed), ..
    } = s
    {
        *boxed
    } else if let Stmt::ExportDecl { inner: None, .. } = s {
        // P13-S4b — a side-effect import binds no names, so the
        // export FACE of a bare named export (`export { a }`) has no
        // consumer here; the decls it lists already injected as
        // ordinary top-level statements above. Drop the face.
        return Ok(());
    } else {
        s
    };
    // Whole statements inject in source order, but a DECL an earlier
    // request already materialized (or that a later one will ask for
    // by name) goes through the ledger like every other lane's.
    if let Some(n) = decl_name(&stmt)
        && !req.injected.insert(match stmt {
            Stmt::TypeDecl { .. } => super::types_lane::type_key(&n),
            _ => n,
        })
    {
        return Ok(());
    }
    injections.push(stmt);
    Ok(())
}

/// Side-effect imports forward whole statements, nested imports /
/// re-exports queue, export forms inject (see each arm).
pub(super) fn walk_lib_stmt(
    ast: &mut Ast,
    work: &mut VecDeque<WorkItem>,
    target_dir: &Path,
    injections: &mut Vec<Stmt>,
    s: Stmt,
    req: &mut LibRequest<'_>,
) -> Result<(), String> {
    // An erased type-only clause reaches its module on the type lane.
    if let Stmt::ImportDecl {
        source,
        type_only: true,
        ..
    }
    | Stmt::ExportDecl {
        source: Some(source),
        type_only: true,
        ..
    } = &s
    {
        return super::types_lane::queue_types(work, target_dir, source);
    }
    // Side-effect-only import — see [`inject_side_effect_stmt`].
    if req.side_effect_only {
        return inject_side_effect_stmt(work, target_dir, injections, s, req);
    }
    match s {
        Stmt::ImportDecl {
            source,
            named,
            default,
            namespace,
            ..
        } => {
            req.ledger.import_clause(&source, &named, &default);
            queue_nested_import(work, target_dir, &source, named, default, namespace)?;
        }
        Stmt::ExportDecl {
            inner: None,
            default_expr: Some(eid),
            ..
        } => {
            // `export default <expr>` form. Inject as a synthetic
            // `let <binding> = <expr>` if the importer used the
            // default-binding form (`import x from ...`) — or if a
            // namespace request synthesized a `__nsdefault_<alias>`
            // binding for its `default` field (§16.2.1.10; minted in
            // `resolve_imports` when the lib has a default export).
            if let Some(alias) = req.default_alias {
                injections.push(Stmt::LetDecl {
                    mutable: false,
                    name: alias.clone(),
                    type_ann: None,
                    init: eid,
                    is_var: false,
                });
                if let Some(ns) = req.ns.as_deref_mut() {
                    ns.claim("default", alias);
                }
            }
            // If no binding was requested the export is silently
            // dropped — matches "named exports not in the
            // importer's list" behavior for parity.
        }
        Stmt::ExportDecl {
            inner: Some(boxed), ..
        } => {
            inject_export_inner(
                ast,
                injections,
                *boxed,
                req.want,
                req.rename,
                req.ns.as_deref_mut(),
                req.injected,
                req.demangle,
                req.hidden,
                &mut req.landings,
            );
        }
        Stmt::ExportDecl {
            inner: None,
            star: Some(star),
            source: Some(lib_source),
            ..
        } => {
            queue_star_reexport(
                work,
                target_dir,
                &star,
                &lib_source,
                req.want,
                req.rename,
                req.default_alias,
                req.own_exports,
                req.ns.as_deref_mut(),
            )?;
        }
        Stmt::ExportDecl {
            inner: None,
            named: lib_named,
            source: Some(lib_source),
            ..
        } => {
            queue_reexport(
                work,
                target_dir,
                &lib_named,
                &lib_source,
                req.want,
                req.rename,
                req.default_alias,
                req.ns.as_deref_mut(),
                &mut req.ledger,
            )?;
        }
        Stmt::ExportDecl { inner: None, .. } => {
            // P13-S4b — the bare named export FACE: its
            // (orig, exported) pairs were collected before
            // the walk; the decls themselves inject below.
        }
        other => {
            // A statement that declares nothing is module EVALUATION,
            // not a binding: a candidate's runs at its `import()`; a
            // `Multi` (a destructuring declaration's spelling) binds
            // and stays with the decl lane.
            if !matches!(other, Stmt::Multi(_))
                && decl_name(&other).is_none()
                && let Some(deferred) = req.deferred.as_deref_mut()
            {
                deferred.push(other);
                return Ok(());
            }
            if req.evaluate && decl_name(&other).is_none() {
                injections.push(other);
                return Ok(());
            }
            inject_bare_exported_decl(
                ast,
                injections,
                other,
                req.bare_exports,
                req.want,
                req.rename,
                req.default_alias,
                req.ns.as_deref_mut(),
                req.injected,
                req.demangle,
                req.hidden,
                &mut req.landings,
            );
        }
    }
    Ok(())
}
