//! `Formatter::fmt_stmt` — per-Stmt emission walker for `tr fmt`.
//! Each match arm emits source text for one Stmt kind via the
//! surrounding `Formatter`'s write / write_indent / fmt_expr /
//! fmt_stmt (recursive) primitives; the larger arms (If / For /
//! ForOfSplitIter / Try / Switch / LetDecl) delegate to per-arm
//! sibling fns below, the shared `{ ... }` brace-block shape lives
//! in [`Formatter::fmt_block_braces`], and the signature trio
//! (class-method head / type-params / params) in `fmt_stmt_sigs`.
//!
//! Extracted from `formatter.rs` (2026-05-25, god-file decomp batch 18).

use crate::ast::{Expr, ExprId, Stmt};

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
                self.fmt_let_decl_body(*mutable, name, type_ann.as_deref(), *init, *is_var);
            }
            Stmt::UsingDecl {
                name,
                type_ann,
                init,
                is_await,
            } => self.fmt_using_decl(name, type_ann.as_deref(), *init, *is_await),
            Stmt::Return(opt) => self.fmt_return(*opt),
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
            Stmt::Break(label) => self.fmt_break_or_continue("break", label),
            Stmt::Continue(label) => self.fmt_break_or_continue("continue", label),
            Stmt::Labeled { label, body } => {
                self.write_indent();
                self.write(&format!("{label}:"));
                self.newline();
                self.fmt_stmt(body);
            }
            Stmt::Block(stmts) => {
                self.write_indent();
                self.fmt_block_braces(stmts);
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
            } => self.fmt_if(*cond, then_branch, else_branch.as_deref()),
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
            } => self.fmt_for(init.as_deref(), *cond, *step, body),
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => self.fmt_for_of_split_iter(var_name, *parent, *sep, body),
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
            } => self.fmt_try(
                body,
                *had_catch,
                catch_param.as_deref(),
                catch_type.as_deref(),
                catch_body,
                finally_body.as_deref(),
            ),
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => self.fmt_switch(*scrutinee, cases, default.as_deref()),
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                is_generator,
                span: _,
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
                *parent,
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
                source,
            } => self.fmt_export_decl(inner.as_deref(), named, *default_expr, source.as_deref()),
        }
    }

    /// `break` / `continue`, with an optional label (ES §14.8/§14.9).
    fn fmt_break_or_continue(&mut self, kw: &str, label: &Option<String>) {
        self.write_indent();
        match label {
            Some(l) => self.write(&format!("{kw} {l}")),
            None => self.write(kw),
        }
    }

    /// `Stmt::If` arm body — `if (c) { ... }` with `else if` chains
    /// emitted inline.
    fn fmt_if(&mut self, cond: ExprId, then_branch: &Stmt, else_branch: Option<&Stmt>) {
        self.write_indent();
        self.write("if (");
        self.fmt_expr(cond);
        self.write(") ");
        self.fmt_braced_or_inline(then_branch);
        if let Some(eb) = else_branch {
            self.write(" else ");
            if matches!(eb, Stmt::If { .. }) {
                // `else if` chain: emit the nested If inline.
                self.fmt_stmt_inline(eb);
            } else {
                self.fmt_braced_or_inline(eb);
            }
        }
    }

    /// `Stmt::For` arm body — classic three-slot `for (init; cond;
    /// step) body`, each slot optional.
    fn fmt_for(
        &mut self,
        init: Option<&Stmt>,
        cond: Option<ExprId>,
        step: Option<ExprId>,
        body: &Stmt,
    ) {
        self.write_indent();
        self.write("for (");
        if let Some(i) = init {
            self.fmt_for_init(i);
        }
        self.write("; ");
        if let Some(c) = cond {
            self.fmt_expr(c);
        }
        self.write("; ");
        if let Some(st) = step {
            self.fmt_expr(st);
        }
        self.write(") ");
        self.fmt_braced_or_inline(body);
    }

    /// `Stmt::ForOfSplitIter` arm body — format back to source-level
    /// `for (let v of x.split(s)) body` since that's what the user
    /// wrote pre-parser-rewrite.
    fn fmt_for_of_split_iter(&mut self, var_name: &str, parent: ExprId, sep: ExprId, body: &Stmt) {
        self.write_indent();
        self.write("for (let ");
        self.write(var_name);
        self.write(" of ");
        self.fmt_expr(parent);
        self.write(".split(");
        self.fmt_expr(sep);
        self.write(")) ");
        self.fmt_braced_or_inline(body);
    }

    /// Shared LetDecl emission body (no leading indent) — used by the
    /// `Stmt::Return` arm — `return( expr)?`.
    fn fmt_return(&mut self, opt: Option<ExprId>) {
        self.write_indent();
        self.write("return");
        if let Some(eid) = opt {
            self.write(" ");
            self.fmt_expr(eid);
        }
    }

    /// `Stmt::UsingDecl` arm — `using x(: T)? = init;` (RFC
    /// 20260809 B1; the formatter sees the variant only when running
    /// on a raw pre-desugar tree).
    fn fmt_using_decl(&mut self, name: &str, type_ann: Option<&str>, init: ExprId, is_await: bool) {
        self.write_indent();
        if is_await {
            self.write("await ");
        }
        self.write("using ");
        self.write(name);
        if let Some(t) = type_ann {
            self.write(": ");
            self.write(t);
        }
        self.write(" = ");
        self.fmt_expr(init);
        self.write(";");
    }

    /// `Stmt::LetDecl` arm and [`Self::fmt_for_init`].
    /// `var` must format as `var` — emitting let/const here silently
    /// rewrote `var x` decls (zero-warn surfaced it).
    fn fmt_let_decl_body(
        &mut self,
        mutable: bool,
        name: &str,
        type_ann: Option<&str>,
        init: ExprId,
        is_var: bool,
    ) {
        self.write(if is_var {
            "var "
        } else if mutable {
            "let "
        } else {
            "const "
        });
        self.write(name);
        if let Some(ann) = type_ann {
            self.write(": ");
            self.write(ann);
        }
        if !matches!(self.ast.get_expr(init), Expr::Uninit) {
            self.write(" = ");
            self.fmt_expr(init);
        }
    }

    /// Emit `{` + newline, the stmt list one level deeper, then the
    /// closing `}` at the current indent — the brace-block shape
    /// shared by the Block arm, try/catch/finally, braced bodies,
    /// class-method bodies and the ArrowFn block body (fmt_expr).
    pub(super) fn fmt_block_braces(&mut self, stmts: &[Stmt]) {
        self.write("{");
        self.newline();
        self.indent += 1;
        for s in stmts {
            if self.is_synth_strict_directive(s) {
                continue;
            }
            self.fmt_stmt(s);
            self.newline();
        }
        self.indent -= 1;
        self.write_indent();
        self.write("}");
    }

    /// `Stmt::Try` arm body — `try { ... } catch (p: T) { ... }
    /// finally { ... }` with catch/finally sections optional.
    fn fmt_try(
        &mut self,
        body: &[Stmt],
        had_catch: bool,
        catch_param: Option<&str>,
        catch_type: Option<&str>,
        catch_body: &[Stmt],
        finally_body: Option<&[Stmt]>,
    ) {
        self.write_indent();
        self.write("try ");
        self.fmt_block_braces(body);
        if had_catch {
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
            self.write(" ");
            self.fmt_block_braces(catch_body);
        }
        if let Some(fb) = finally_body {
            self.write(" finally ");
            self.fmt_block_braces(fb);
        }
    }

    /// `Stmt::Switch` arm body — case bodies are indented one level
    /// under their `case x:` label, no braces.

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
            } => self.fmt_let_decl_body(*mutable, name, type_ann.as_deref(), *init, *is_var),
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
            self.fmt_block_braces(stmts);
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
}
