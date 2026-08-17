//! Injection splice — the final "prepend the resolver's output to the
//! entry" step and its span-reset walk, split from `modules.rs` when
//! the 423-01 deconflict wiring pushed it past the 500-line limit.

use crate::ast::{Ast, Stmt};

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
