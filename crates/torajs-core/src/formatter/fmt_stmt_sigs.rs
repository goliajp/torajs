//! `Formatter::fmt_{class_method,type_params,params}` — the
//! signature-shaped emission trio (a class member head, a `<T, U>`
//! type-parameter list, a `(a: T, b = e)` parameter list) shared by
//! the stmt walker and the decl helpers. Split from
//! `formatter/fmt_stmt.rs` when rotations 341-342 pushed it past the
//! 500-line cap (multi-impl-blocks pattern, `fmt_stmt_decls` posture).

use crate::ast::{ClassMethod, Param, Visibility};

use super::Formatter;

impl<'a> Formatter<'a> {
    pub(super) fn fmt_class_method(&mut self, m: &ClassMethod, is_static: bool) {
        self.write_indent();
        match m.visibility {
            Visibility::Private => self.write("private "),
            Visibility::Protected => self.write("protected "),
            Visibility::Public => {}
        }
        if is_static {
            self.write("static ");
        }
        if m.is_abstract {
            self.write("abstract ");
        }
        self.write(&m.name);
        self.fmt_params(&m.params);
        if let Some(ret) = &m.return_type {
            self.write(": ");
            self.write(ret);
        }
        if m.is_abstract {
            // No body for abstract methods — written as `abstract m(): T`
            return;
        }
        self.write(" ");
        self.fmt_block_braces(&m.body);
    }

    pub(super) fn fmt_type_params(&mut self, tp: &[String]) {
        if tp.is_empty() {
            return;
        }
        self.write("<");
        for (i, t) in tp.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(t);
        }
        self.write(">");
    }

    pub(super) fn fmt_params(&mut self, params: &[Param]) {
        self.write("(");
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            if p.is_rest {
                self.write("...");
            }
            self.write(&p.name);
            if let Some(ann) = &p.type_ann {
                self.write(": ");
                self.write(ann);
            }
            if let Some(deid) = p.default {
                self.write(" = ");
                self.fmt_expr(deid);
            }
        }
        self.write(")");
    }
}
