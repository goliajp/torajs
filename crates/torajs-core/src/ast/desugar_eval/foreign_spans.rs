//! Source spans on statements that came out of an eval text.
//!
//! `Stmt::FnDecl` carries the byte range of the function's own source,
//! and everything downstream slices `Ast::source` with it — the
//! type-erasure pass behind `Function.prototype.toString`
//! (`fn_source_erase::erase_types`), and the registry row it interns.
//! A function declared inside an eval has a range into the EVAL TEXT,
//! which is a different string: slicing the program with it answers
//! another function's characters, or lands mid-UTF-8 and panics the
//! compiler (`end byte index N is not a char boundary`).
//!
//! `Span { start: 0, end: 0 }` is already the codebase's "no source
//! text" sentinel — synthesized declarations use it, and the consumers
//! answer `function f() { [native code] }` / a NULL `src_ptr` for it.
//! That is the honest answer here too: tr has the text, but not in the
//! buffer these consumers read. §20.2.3.5 wants the eval text itself,
//! which needs a per-function source buffer and is a separate surface;
//! until it exists, `q.toString()` on an eval-declared `q` answers the
//! sentinel form, and that form prints the LIFTED name
//! (`function __nested___top_q_0() { [native code] }`) because nothing
//! demangles `__nested_<parent>_<name>_<uid>`. Both are wrong; a
//! sentinel is the less wrong of the two, since the alternative was a
//! slice of unrelated characters out of the program.
//!
//! Same family as the `class_decl_spans` snapshot in
//! `parser::parse_into_eval` (r420-06), which fixed exactly this for
//! class spans and left the fn ones. Imported modules have the defect
//! through the third door — their spans index their own file — and are
//! not touched here.

use super::super::Stmt;
use crate::lexer::Span;

/// Blank every `FnDecl` span in `stmts`, at any depth: a nested
/// function's range is just as foreign as its enclosure's.
pub(super) fn blank_fn_spans(stmts: &mut [Stmt]) {
    for s in stmts.iter_mut() {
        blank_stmt(s);
    }
}

fn blank_stmt(s: &mut Stmt) {
    match s {
        Stmt::FnDecl { span, body, .. } => {
            *span = Span { start: 0, end: 0 };
            blank_fn_spans(body);
        }
        Stmt::Block(b) | Stmt::Multi(b) => blank_fn_spans(b),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            blank_stmt(then_branch);
            if let Some(e) = else_branch.as_deref_mut() {
                blank_stmt(e);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => blank_stmt(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                blank_stmt(i);
            }
            blank_stmt(body);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            blank_fn_spans(body);
            blank_fn_spans(catch_body);
            if let Some(f) = finally_body.as_mut() {
                blank_fn_spans(f);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                blank_fn_spans(&mut c.body);
            }
            if let Some(d) = default.as_mut() {
                blank_fn_spans(d);
            }
        }
        _ => {}
    }
}
