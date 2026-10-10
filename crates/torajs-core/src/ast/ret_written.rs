//! Which functions the program wrote a return type for
//! (`Ast::ret_written`). An object return type the compiler infers for
//! a function returning an object literal is spelled the same as a
//! written object type literal (`__inlobj(..)`), and only a written one
//! is a checked cast. The parse is the one point where the two still
//! differ (`return_type` is `Some` exactly when the program wrote one),
//! so it is recorded here, by the source span every later rewrite of
//! the function carries onto its declaration (`Stmt::FnDecl::span`, an
//! arrow's `expr_spans` entry once lifted).

use super::desugar_with::walk::stmt_children_ref;
use super::{Ast, Expr, StaticInit, Stmt};
use crate::lexer::Span;

impl Ast {
    /// Whether the program wrote the return type of the function whose
    /// declaration carries `span`. A synthesized one (no span) did not.
    pub(crate) fn ret_written_at(&self, span: Span) -> bool {
        (self.ret_written.get(&(span.start, span.end))).is_some_and(|w| *w)
    }

    /// Whether the program wrote the return type of the function `name`
    /// declared at `span`, a monomorph clone answering for its original.
    pub(crate) fn fn_ret_written(&self, name: &str, span: Span) -> bool {
        self.ret_written_at(span) || self.ret_written_clones.contains(name)
    }

    /// A monomorph clone of `original` (a top-level declaration) keeps
    /// whether its return type was written.
    pub(crate) fn mirror_ret_written(&mut self, original: &str, clone: &str) {
        let written = self.stmts.iter().any(|s| {
            matches!(s, Stmt::FnDecl { name, span, .. }
                if name == original && self.fn_ret_written(name, *span))
        });
        if written {
            self.ret_written_clones.insert(clone.to_string());
        }
    }
}

/// Record every function form this parse added (statements from
/// `stmt_offset`, expressions from `id_offset`). A nested parse (an
/// imported module, a lib or eval source) has spans into another text,
/// which may coincide with the entry's; its spans read as not written.
pub(crate) fn record_written_returns(ast: &mut Ast, stmt_offset: usize, id_offset: usize) {
    let nested = stmt_offset > 0 || id_offset > 0;
    let mut found = Vec::new();
    visit(&ast.stmts[stmt_offset..], &mut found);
    for (i, e) in ast.exprs.iter().enumerate().skip(id_offset) {
        if let Expr::ArrowFn {
            return_type, body, ..
        } = e
        {
            found.push((ast.expr_spans[i], return_type.is_some()));
            visit(body, &mut found);
        }
    }
    for (span, written) in found {
        if span.start == 0 && span.end == 0 {
            continue;
        }
        // two forms sharing a span (a parse-time rewrite carrying the
        // user's range) count as written only when both are
        let written = written && !nested;
        (ast.ret_written.entry((span.start, span.end)))
            .and_modify(|w| *w &= written)
            .or_insert(written);
    }
}

/// The function declarations and methods among `stmts` and nested in
/// them; arrows are reached through the expression arena instead.
fn visit(stmts: &[Stmt], found: &mut Vec<(Span, bool)>) {
    for s in stmts {
        match s {
            Stmt::FnDecl {
                return_type,
                body,
                span,
                ..
            } => {
                found.push((*span, return_type.is_some()));
                visit(body, found);
            }
            Stmt::ClassDecl {
                static_init,
                ctor,
                methods,
                static_methods,
                ..
            } => {
                for m in methods.iter().chain(static_methods) {
                    found.push((m.span, m.return_type.is_some()));
                    visit(&m.body, found);
                }
                if let Some(c) = ctor {
                    visit(&c.body, found);
                }
                for init in static_init {
                    if let StaticInit::Block(b) = init {
                        visit(b, found);
                    }
                }
            }
            Stmt::ExportDecl { inner: Some(d), .. } => visit(std::slice::from_ref(d), found),
            _ => {
                for c in stmt_children_ref(s) {
                    visit(std::slice::from_ref(c), found);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::tokenize;
    use crate::parser::{parse, parse_into};

    const SRC: &str = "function a(): { x: number } { return { x: 1 }; }
function b() { return { x: 1 }; }
const c = (): { x: number } => ({ x: 1 });
const d = () => ({ x: 1 });
class K { m(): { x: number } { return { x: 1 }; } n() { return { x: 1 }; } }
const o = { p(): { x: number } { return { x: 1 }; }, q() { return { x: 1 }; } };
";

    #[test]
    fn every_form_records_whether_its_return_type_was_written() {
        let ast = parse(SRC, &tokenize(SRC).unwrap()).unwrap();
        assert_eq!(ast.ret_written.len(), 8);
        for (&(start, end), &written) in &ast.ret_written {
            let text = &SRC[start as usize..end as usize];
            assert_eq!(written, text.contains("): {"), "{text}");
        }
        assert_eq!(ast.ret_written.values().filter(|w| **w).count(), 4);
    }

    #[test]
    fn a_nested_parse_records_nothing_as_written() {
        let mut ast = parse(SRC, &tokenize(SRC).unwrap()).unwrap();
        parse_into(SRC, &tokenize(SRC).unwrap(), &mut ast).unwrap();
        assert_eq!(ast.ret_written.len(), 8);
        assert!(ast.ret_written.values().all(|w| !*w));
    }
}
