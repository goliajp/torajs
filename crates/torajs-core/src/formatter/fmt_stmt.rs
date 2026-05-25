//! `Formatter::fmt_stmt` — per-Stmt emission walker for `tr fmt`.
//!
//! CARVE-OUT (file-size.md §Carve-out #2 dispatch table): this file
//! is a data-driven match-arm-per-Stmt-variant dispatcher. Each arm
//! emits source text for one Stmt kind via the surrounding
//! `Formatter`'s write / write_indent / fmt_expr / fmt_stmt
//! (recursive) primitives. Length comes from variant count × arm
//! body, not from logic complexity. Sub-split per variant is a
//! follow-up batch when individual arms grow further.
//!
//! Extracted from `formatter.rs` (2026-05-25, god-file decomp batch 18).

use crate::ast::{ClassMethod, Expr, Param, Stmt, Visibility};

use super::Formatter;

impl<'a> Formatter<'a> {
    pub(super) fn fmt_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(eid) => {
                self.write_indent();
                self.fmt_expr(*eid);
            }
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var,
            } => {
                self.write_indent();
                // `var` must format as `var` — emitting let/const here
                // silently rewrote `var x` decls (zero-warn surfaced it).
                self.write(if *is_var {
                    "var "
                } else if *mutable {
                    "let "
                } else {
                    "const "
                });
                self.write(name);
                if let Some(ann) = type_ann {
                    self.write(": ");
                    self.write(ann);
                }
                if !matches!(self.ast.get_expr(*init), Expr::Uninit) {
                    self.write(" = ");
                    self.fmt_expr(*init);
                }
            }
            Stmt::Return(opt) => {
                self.write_indent();
                self.write("return");
                if let Some(eid) = opt {
                    self.write(" ");
                    self.fmt_expr(*eid);
                }
            }
            Stmt::Yield(eid) => {
                self.write_indent();
                self.write("yield ");
                self.fmt_expr(*eid);
            }
            Stmt::YieldInto {
                var,
                type_ann,
                value,
            } => {
                self.write_indent();
                self.write("let ");
                self.write(var);
                if let Some(ann) = type_ann {
                    self.write(": ");
                    self.write(ann);
                }
                self.write(" = yield ");
                self.fmt_expr(*value);
            }
            Stmt::Throw(eid) => {
                self.write_indent();
                self.write("throw ");
                self.fmt_expr(*eid);
            }
            Stmt::Break => {
                self.write_indent();
                self.write("break");
            }
            Stmt::Continue => {
                self.write_indent();
                self.write("continue");
            }
            Stmt::Block(stmts) => {
                self.write_indent();
                self.write("{");
                self.newline();
                self.indent += 1;
                for s in stmts {
                    self.fmt_stmt(s);
                    self.newline();
                }
                self.indent -= 1;
                self.write_indent();
                self.write("}");
            }
            Stmt::Multi(stmts) => {
                // Compiler-synthesized; flatten to back-to-back stmts
                // at the current indent. Should not appear in a
                // pre-desugar AST, but tolerate it for safety.
                for (i, s) in stmts.iter().enumerate() {
                    if i > 0 {
                        self.newline();
                    }
                    self.fmt_stmt(s);
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.write_indent();
                self.write("if (");
                self.fmt_expr(*cond);
                self.write(") ");
                self.fmt_braced_or_inline(then_branch);
                if let Some(eb) = else_branch {
                    self.write(" else ");
                    if matches!(eb.as_ref(), Stmt::If { .. }) {
                        // `else if` chain: emit the nested If inline.
                        self.fmt_stmt_inline(eb);
                    } else {
                        self.fmt_braced_or_inline(eb);
                    }
                }
            }
            Stmt::While { cond, body } => {
                self.write_indent();
                self.write("while (");
                self.fmt_expr(*cond);
                self.write(") ");
                self.fmt_braced_or_inline(body);
            }
            Stmt::DoWhile { body, cond } => {
                self.write_indent();
                self.write("do ");
                self.fmt_braced_or_inline(body);
                self.write(" while (");
                self.fmt_expr(*cond);
                self.write(")");
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                self.write_indent();
                self.write("for (");
                if let Some(i) = init {
                    self.fmt_for_init(i);
                }
                self.write("; ");
                if let Some(c) = cond {
                    self.fmt_expr(*c);
                }
                self.write("; ");
                if let Some(st) = step {
                    self.fmt_expr(*st);
                }
                self.write(") ");
                self.fmt_braced_or_inline(body);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                // Format back to source-level `for (let v of x.split(s)) body`
                // since that's what the user wrote pre-parser-rewrite.
                self.write_indent();
                self.write("for (let ");
                self.write(var_name);
                self.write(" of ");
                self.fmt_expr(*parent);
                self.write(".split(");
                self.fmt_expr(*sep);
                self.write(")) ");
                self.fmt_braced_or_inline(body);
            }
            Stmt::ForOf {
                var_name,
                var_type_ann,
                src_ident,
                body,
                ..
            } => {
                self.write_indent();
                self.write("for (let ");
                self.write(var_name);
                if let Some(ann) = var_type_ann {
                    self.write(": ");
                    self.write(ann);
                }
                self.write(" of ");
                self.write(src_ident);
                self.write(") ");
                self.fmt_braced_or_inline(body);
            }
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                self.write_indent();
                self.write("try {");
                self.newline();
                self.indent += 1;
                for s in body {
                    self.fmt_stmt(s);
                    self.newline();
                }
                self.indent -= 1;
                self.write_indent();
                self.write("}");
                if *had_catch {
                    self.write(" catch");
                    if let Some(p) = catch_param {
                        self.write(" (");
                        self.write(p);
                        if let Some(ty) = catch_type {
                            self.write(": ");
                            self.write(ty);
                        }
                        self.write(")");
                    }
                    self.write(" {");
                    self.newline();
                    self.indent += 1;
                    for s in catch_body {
                        self.fmt_stmt(s);
                        self.newline();
                    }
                    self.indent -= 1;
                    self.write_indent();
                    self.write("}");
                }
                if let Some(fb) = finally_body {
                    self.write(" finally {");
                    self.newline();
                    self.indent += 1;
                    for s in fb {
                        self.fmt_stmt(s);
                        self.newline();
                    }
                    self.indent -= 1;
                    self.write_indent();
                    self.write("}");
                }
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                self.write_indent();
                self.write("switch (");
                self.fmt_expr(*scrutinee);
                self.write(") {");
                self.newline();
                self.indent += 1;
                for case in cases {
                    self.write_indent();
                    self.write("case ");
                    self.fmt_expr(case.value);
                    self.write(":");
                    self.newline();
                    self.indent += 1;
                    for s in &case.body {
                        self.fmt_stmt(s);
                        self.newline();
                    }
                    self.indent -= 1;
                }
                if let Some(d) = default {
                    self.write_indent();
                    self.write("default:");
                    self.newline();
                    self.indent += 1;
                    for s in d {
                        self.fmt_stmt(s);
                        self.newline();
                    }
                    self.indent -= 1;
                }
                self.indent -= 1;
                self.write_indent();
                self.write("}");
            }
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                is_generator,
            } => self.fmt_fn_decl(
                name,
                type_params,
                params,
                return_type.as_deref(),
                body,
                *is_generator,
            ),
            Stmt::TypeDecl {
                name,
                type_params,
                fields,
            } => self.fmt_type_decl(name, type_params, fields),
            Stmt::ClassDecl {
                name,
                type_params,
                parent,
                is_abstract,
                fields,
                static_init,
                ctor,
                methods,
                static_methods,
            } => self.fmt_class_decl(
                name,
                type_params,
                parent.as_deref(),
                *is_abstract,
                fields,
                static_init,
                ctor.as_ref(),
                methods,
                static_methods,
            ),
            Stmt::ImportDecl {
                default,
                namespace,
                named,
                source,
            } => self.fmt_import_decl(default.as_deref(), namespace.as_deref(), named, source),
            Stmt::ExportDecl {
                inner,
                named,
                default_expr,
            } => self.fmt_export_decl(inner.as_deref(), named, *default_expr),
        }
    }

    fn fmt_for_init(&mut self, s: &Stmt) {
        // `for (init; ...)` accepts a LetDecl or an ExprStmt as init.
        // Reuse the regular Stmt formatter but with indent suppressed.
        match s {
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var,
            } => {
                // `var` must format as `var` (zero-warn surfaced this
                // arm silently rewriting `var` → let/const).
                self.write(if *is_var {
                    "var "
                } else if *mutable {
                    "let "
                } else {
                    "const "
                });
                self.write(name);
                if let Some(ann) = type_ann {
                    self.write(": ");
                    self.write(ann);
                }
                if !matches!(self.ast.get_expr(*init), Expr::Uninit) {
                    self.write(" = ");
                    self.fmt_expr(*init);
                }
            }
            Stmt::Expr(eid) => self.fmt_expr(*eid),
            other => panic!("not yet supported: fmt(for-init {other:?})"),
        }
    }

    fn fmt_braced_or_inline(&mut self, s: &Stmt) {
        // If the body is a Block, emit it as `{ ... }` on the same
        // line as the keyword. Otherwise emit a single-line stmt
        // wrapped in braces (tr-fmt's opinionated choice — no
        // single-stmt-no-braces shape, matches prettier).
        if let Stmt::Block(stmts) = s {
            self.write("{");
            self.newline();
            self.indent += 1;
            for s in stmts {
                self.fmt_stmt(s);
                self.newline();
            }
            self.indent -= 1;
            self.write_indent();
            self.write("}");
        } else {
            self.write("{");
            self.newline();
            self.indent += 1;
            self.fmt_stmt(s);
            self.newline();
            self.indent -= 1;
            self.write_indent();
            self.write("}");
        }
    }

    fn fmt_stmt_inline(&mut self, s: &Stmt) {
        // Emit a stmt without leading indent (used for `else if`).
        let saved = self.indent;
        self.indent = 0;
        self.fmt_stmt(s);
        self.indent = saved;
    }

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
        self.write(" {");
        self.newline();
        self.indent += 1;
        for s in &m.body {
            self.fmt_stmt(s);
            self.newline();
        }
        self.indent -= 1;
        self.write_indent();
        self.write("}");
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
