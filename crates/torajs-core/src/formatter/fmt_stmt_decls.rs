//! `Formatter::fmt_{fn,type,class,import,export}_decl` — helpers
//! invoked from the top-level `fmt_stmt` dispatcher (in
//! `formatter/fmt_stmt.rs`) for every `Stmt` variant whose body
//! exceeded ~30 LOC inline. Per Rust's multi-impl-blocks pattern,
//! all five live in their own `impl Formatter` block here so the
//! dispatcher file stays at one variant ≈ one line.
//!
//! Extracted from `fmt_stmt.rs` (2026-05-25, god-file decomp batch 19).

use crate::ast::{ClassCtor, ClassMethod, ExprId, Param, StaticInit, Stmt};

use super::Formatter;

impl<'a> Formatter<'a> {
    pub(super) fn fmt_fn_decl(
        &mut self,
        name: &str,
        type_params: &[String],
        params: &[Param],
        return_type: Option<&str>,
        body: &[Stmt],
        is_generator: bool,
    ) {
        self.write_indent();
        self.write("function");
        if is_generator {
            self.write("*");
        }
        self.write(" ");
        self.write(name);
        self.fmt_type_params(type_params);
        self.fmt_params(params);
        if let Some(ret) = return_type {
            self.write(": ");
            self.write(ret);
        }
        self.write(" {");
        self.newline();
        self.indent += 1;
        for s in body {
            self.fmt_stmt(s);
            self.newline();
        }
        self.indent -= 1;
        self.write_indent();
        self.write("}");
    }

    pub(super) fn fmt_type_decl(
        &mut self,
        name: &str,
        type_params: &[String],
        fields: &[(String, String)],
    ) {
        self.write_indent();
        self.write("type ");
        self.write(name);
        self.fmt_type_params(type_params);
        self.write(" = { ");
        for (i, (fn_, fty)) in fields.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(fn_);
            self.write(": ");
            self.write(fty);
        }
        self.write(" }");
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn fmt_class_decl(
        &mut self,
        name: &str,
        type_params: &[String],
        parent: Option<&str>,
        is_abstract: bool,
        fields: &[(String, String)],
        static_init: &[StaticInit],
        ctor: Option<&ClassCtor>,
        methods: &[ClassMethod],
        static_methods: &[ClassMethod],
    ) {
        self.write_indent();
        if is_abstract {
            self.write("abstract ");
        }
        self.write("class ");
        self.write(name);
        self.fmt_type_params(type_params);
        if let Some(p) = parent {
            self.write(" extends ");
            self.write(p);
        }
        self.write(" {");
        self.newline();
        self.indent += 1;
        for (fn_, ann) in fields {
            self.write_indent();
            self.write(fn_);
            self.write(": ");
            self.write(ann);
            self.newline();
        }
        for si in static_init {
            self.write_indent();
            match si {
                StaticInit::Field(sf) => {
                    self.write("static ");
                    self.write(&sf.name);
                    self.write(": ");
                    self.write(&sf.type_ann);
                    self.write(" = ");
                    self.fmt_expr(sf.init);
                    self.newline();
                }
                StaticInit::Block(stmts) => {
                    self.write("static {");
                    self.newline();
                    self.indent += 1;
                    for s in stmts {
                        self.fmt_stmt(s);
                    }
                    self.indent -= 1;
                    self.write_indent();
                    self.write("}");
                    self.newline();
                }
            }
        }
        if let Some(ctor) = ctor {
            if !fields.is_empty() || !static_init.is_empty() {
                self.newline();
            }
            self.write_indent();
            self.write("constructor");
            self.fmt_params(&ctor.params);
            self.write(" {");
            self.newline();
            self.indent += 1;
            for s in &ctor.body {
                self.fmt_stmt(s);
                self.newline();
            }
            self.indent -= 1;
            self.write_indent();
            self.write("}");
            self.newline();
        }
        for m in methods {
            self.newline();
            self.fmt_class_method(m, false);
            self.newline();
        }
        for m in static_methods {
            self.newline();
            self.fmt_class_method(m, true);
            self.newline();
        }
        self.indent -= 1;
        self.write_indent();
        self.write("}");
    }

    pub(super) fn fmt_import_decl(
        &mut self,
        default: Option<&str>,
        namespace: Option<&str>,
        named: &[(String, Option<String>)],
        source: &str,
    ) {
        self.write_indent();
        self.write("import ");
        let mut wrote_clause = false;
        if let Some(d) = default {
            self.write(d);
            wrote_clause = true;
        }
        if let Some(ns) = namespace {
            if wrote_clause {
                self.write(", ");
            }
            self.write("* as ");
            self.write(ns);
            wrote_clause = true;
        }
        if !named.is_empty() {
            if wrote_clause {
                self.write(", ");
            }
            self.write("{ ");
            for (i, (n, alias)) in named.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(n);
                if let Some(a) = alias {
                    self.write(" as ");
                    self.write(a);
                }
            }
            self.write(" }");
            wrote_clause = true;
        }
        if wrote_clause {
            self.write(" from ");
        }
        self.write("'");
        self.write(source);
        self.write("'");
    }

    pub(super) fn fmt_export_decl(
        &mut self,
        inner: Option<&Stmt>,
        named: &[(String, Option<String>)],
        default_expr: Option<ExprId>,
    ) {
        self.write_indent();
        self.write("export ");
        if let Some(eid) = default_expr {
            self.write("default ");
            self.fmt_expr(eid);
            return;
        }
        if !named.is_empty() {
            self.write("{ ");
            for (i, (n, alias)) in named.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(n);
                if let Some(a) = alias {
                    self.write(" as ");
                    self.write(a);
                }
            }
            self.write(" }");
            return;
        }
        if let Some(s) = inner {
            // Inner stmt already emits its own indent — strip by
            // temporarily clearing indent for the recursive call.
            let saved = self.indent;
            self.indent = 0;
            self.fmt_stmt(s);
            self.indent = saved;
        }
    }
}
