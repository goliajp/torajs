//! Injection splice — the final "prepend the resolver's output to the
//! entry" step and its span-reset walk, split from `modules.rs` when
//! the 423-01 deconflict wiring pushed it past the 500-line limit.

use crate::ast::{Ast, Stmt};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::dyn_import;
use super::graph::ModuleGraph;
use super::resolve_helpers::{NsAccum, inline_dyn_ns_objlits, materialize_pending_namespaces};

/// The resolver's final assembly, moved from `resolve_imports` when
/// its function budget ran out: order the per-path injections by
/// dependency post-order (§16.2.1.5), append the dyn-import
/// dispatcher (426-01 poison verdicts computed here — after every
/// injection is known), materialize the pending namespace objects,
/// inline the dyn-ns object literals, and splice the lot into the
/// entry.
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble_and_splice(
    ast: &mut Ast,
    graph: &ModuleGraph,
    mut injections_by_path: HashMap<PathBuf, Vec<Stmt>>,
    dyn_table: &[(String, String)],
    ns_order: &[String],
    ns_accums: &HashMap<String, NsAccum>,
    dyn_ns_inline: &mut Vec<(String, Vec<(String, String)>)>,
    ambiguous_locals: &HashSet<String>,
) {
    let mut injections: Vec<Stmt> = Vec::new();
    for path in graph.evaluation_order() {
        if let Some(stmts) = injections_by_path.remove(&path) {
            injections.extend(stmts);
        }
    }
    // Dispatcher before `inline_dyn_ns_objlits` — see `synth_dispatcher`.
    if ast.dyn_import_present {
        let poisoned =
            dyn_import::poisoned_candidates(dyn_table, ns_accums, &injections, ambiguous_locals);
        injections.push(dyn_import::synth_dispatcher(ast, dyn_table, &poisoned));
    }
    // The synthetic `let ns = { … }` bindings only reference decls, so
    // they land after every module's statements regardless of which
    // module asked for them.
    materialize_pending_namespaces(ast, &mut injections, ns_order, ns_accums, dyn_ns_inline);
    inline_dyn_ns_objlits(ast, dyn_ns_inline);
    splice_injections(ast, injections);
}

/// Prepend the resolver's injected statements to the entry's own.
/// An injected lib decl's recorded span indexes the LIB file's text,
/// but every span consumer downstream (`intern_fn_source` / the
/// class-method registry) slices the MAIN file's `ast.source`: out of
/// bounds when the main file is shorter, silently wrong toString text
/// otherwise. Reset to the (0,0) "no user source" sentinel — toString
/// answers the native form.
pub(super) fn splice_injections(ast: &mut Ast, injections: Vec<Stmt>) {
    if injections.is_empty() {
        return;
    }
    let mut new_stmts = injections;
    for s in &mut new_stmts {
        clear_injected_spans(s);
    }
    new_stmts.extend(std::mem::take(&mut ast.stmts));
    ast.stmts = new_stmts;
}

/// See the injection-splice comment in [`resolve_imports`] — recurse
/// through nested fn bodies so every registry-visible declaration
/// carries the sentinel.
fn clear_injected_spans(s: &mut Stmt) {
    match s {
        Stmt::FnDecl { span, body, .. } => {
            *span = crate::lexer::Span { start: 0, end: 0 };
            for inner in body {
                clear_injected_spans(inner);
            }
        }
        Stmt::ClassDecl {
            methods,
            static_methods,
            ..
        } => {
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                m.span = crate::lexer::Span { start: 0, end: 0 };
            }
        }
        _ => {}
    }
}
