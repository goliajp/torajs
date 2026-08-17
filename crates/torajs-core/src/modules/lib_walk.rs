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
use super::resolve_helpers::queue_nested_import;
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
    injected: &mut HashSet<String>,
) -> Result<(), String> {
    if let Stmt::ImportDecl {
        source,
        named,
        default,
        namespace,
    } = s
    {
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
        && !injected.insert(n)
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
    // Side-effect-only import — see [`inject_side_effect_stmt`].
    if req.side_effect_only {
        return inject_side_effect_stmt(work, target_dir, injections, s, req.injected);
    }
    match s {
        Stmt::ImportDecl {
            source,
            named,
            default,
            namespace,
        } => {
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
            )?;
        }
        Stmt::ExportDecl { inner: None, .. } => {
            // P13-S4b — the bare named export FACE: its
            // (orig, exported) pairs were collected before
            // the walk; the decls themselves inject below.
        }
        other => {
            inject_bare_exported_decl(
                ast,
                injections,
                other,
                req.bare_exports,
                req.want,
                req.rename,
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
