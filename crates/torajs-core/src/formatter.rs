//! T-05 (v0.3.0) — `tr fmt` deterministic source reformatter.
//!
//! Re-emits valid TypeScript from the parsed AST in tr's canonical
//! style:
//!   - 2-space indent
//!   - single quotes for string literals
//!   - no trailing semicolons
//!   - no config knobs (prettier-shape opinionated; no .prettierrc)
//!
//! ## Comment preservation
//!
//! The lexer drops `//` and `/* */` comments before they reach the
//! AST, so an AST-walking formatter can't see them. To prevent silent
//! comment loss, `format()` runs a comment-detection pre-pass over
//! the raw source; if any line or block comment is found outside a
//! string / regex literal, it returns `FormatError::CommentsPresent`
//! and the CLI exits non-zero with a "comment preservation lands in
//! v0.4 (#N)" message. No `--force` flag is provided — silent
//! comment stripping is the kind of footgun this project's
//! `feedback_no_tech_debt` rule explicitly bans.
//!
//! ## Coverage
//!
//! Covers every Stmt and Expr variant that the conformance / examples
//! corpus uses (LetDecl, FnDecl, ClassDecl, If/While/For/Switch,
//! Return/Throw/Try, all Expr shapes including ObjectLit / ArrayLit
//! / ArrowFn / Closure / Ternary / OptChain / Nullish / PostIncr).
//! Compiler-synthesized shapes (`Stmt::Multi`, `Stmt::ForOfSplitIter`
//! when it predates a desugar pass) are formatted back into their
//! source-level equivalents. Anything genuinely unsupported panics
//! with a `not yet supported: fmt(<variant>)` message — same shape
//! as ssa_lower's panic-hook contract.

use crate::ast::{
    Ast, BinOp, ClassMethod, Expr, ExprId, Param, StaticInit, Stmt, UnaryOp, Visibility,
};
use crate::lexer;
use crate::parser;

#[derive(Debug)]
pub enum FormatError {
    Lex(String),
    Parse(String),
    /// Source contains line or block comments. `format()` refuses to
    /// strip them; comment-aware reformatter is roadmap T-Nfollowup.
    CommentsPresent,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Lex(e) => write!(f, "lex error: {e}"),
            FormatError::Parse(e) => write!(f, "parse error: {e}"),
            FormatError::CommentsPresent => write!(
                f,
                "tr fmt: source contains comments — comment-preserving \
                 reformat is a v0.4 follow-up; refusing to strip them \
                 silently. (Run `tr fmt --strip-comments` once that \
                 flag lands, or pre-strip with another tool.)"
            ),
        }
    }
}

impl std::error::Error for FormatError {}

pub fn format(source: &str) -> Result<String, FormatError> {
    if has_comments_in_code(source) {
        return Err(FormatError::CommentsPresent);
    }
    let tokens = lexer::tokenize(source).map_err(FormatError::Lex)?;
    let ast = parser::parse(&tokens).map_err(FormatError::Parse)?;
    let mut f = Formatter::new(&ast);
    for stmt in &ast.stmts {
        f.fmt_top_stmt(stmt);
    }
    Ok(f.into_output())
}

/// Detect whether source contains TS comments (`//` line or `/* */`
/// block) outside of string / template / regex literals. Mirrors the
/// lexer's tokenization rules well enough to avoid false positives on
/// `"http://..."` / `` `${'/'}` `` / `/[/]/g`. Worst-case false
/// positive flags fmt-able sources as comment-bearing — that's the
/// safe direction.
fn has_comments_in_code(src: &str) -> bool {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        let c = bytes[i];
        match c {
            b'"' | b'\'' => {
                // Plain string literal.
                let q = c;
                i += 1;
                while i < n && bytes[i] != q {
                    if bytes[i] == b'\\' && i + 1 < n {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                i = (i + 1).min(n);
            }
            b'`' => {
                // Template literal. Skip until matching backtick;
                // handle `${ ... }` interpolations by skipping
                // balanced braces (which themselves can contain any
                // shape including comments — we play it safe by
                // counting braces).
                i += 1;
                let mut brace = 0i32;
                while i < n {
                    let b = bytes[i];
                    if b == b'\\' && i + 1 < n {
                        i += 2;
                        continue;
                    }
                    if brace == 0 && b == b'`' {
                        i += 1;
                        break;
                    }
                    if b == b'$' && i + 1 < n && bytes[i + 1] == b'{' {
                        brace += 1;
                        i += 2;
                        continue;
                    }
                    if brace > 0 && b == b'{' {
                        brace += 1;
                    } else if brace > 0 && b == b'}' {
                        brace -= 1;
                    }
                    i += 1;
                }
            }
            b'/' if i + 1 < n => {
                let next = bytes[i + 1];
                if next == b'/' || next == b'*' {
                    return true;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    false
}

struct Formatter<'a> {
    ast: &'a Ast,
    out: String,
    indent: usize,
}

impl<'a> Formatter<'a> {
    fn new(ast: &'a Ast) -> Self {
        Self {
            ast,
            out: String::with_capacity(1024),
            indent: 0,
        }
    }

    fn into_output(self) -> String {
        self.out
    }

    fn write(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
    }

    fn fmt_top_stmt(&mut self, s: &Stmt) {
        self.fmt_stmt(s);
        self.newline();
    }

    fn fmt_stmt(&mut self, s: &Stmt) {
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
            } => {
                self.write_indent();
                self.write("function");
                if *is_generator {
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
            Stmt::TypeDecl {
                name,
                type_params,
                fields,
            } => {
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
            } => {
                self.write_indent();
                if *is_abstract {
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
            Stmt::ImportDecl {
                default,
                namespace,
                named,
                source,
            } => {
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
            Stmt::ExportDecl {
                inner,
                named,
                default_expr,
            } => {
                self.write_indent();
                self.write("export ");
                if let Some(eid) = default_expr {
                    self.write("default ");
                    self.fmt_expr(*eid);
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
                    // Inner stmt already emits its own indent — strip
                    // by re-emitting starting from after the `export `
                    // keyword. Easiest: temporarily clear indent for
                    // the recursive call (the inner won't see the
                    // outer indent bump).
                    let saved = self.indent;
                    self.indent = 0;
                    self.fmt_stmt(s);
                    self.indent = saved;
                }
            }
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

    fn fmt_class_method(&mut self, m: &ClassMethod, is_static: bool) {
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

    fn fmt_type_params(&mut self, tp: &[String]) {
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

    fn fmt_params(&mut self, params: &[Param]) {
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

    fn fmt_expr(&mut self, eid: ExprId) {
        let e = self.ast.get_expr(eid);
        match e {
            Expr::Ident(n) => self.write(n),
            Expr::NewTarget => self.write("new.target"),
            Expr::Number(n) => {
                // Prefer integer form when the f64 round-trips;
                // otherwise %g — mirrors `console.log` semantics.
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                    self.write(&format!("{}", *n as i64));
                } else {
                    self.write(&format!("{n}"));
                }
            }
            Expr::BigInt { digits, radix } => {
                let prefix = match *radix {
                    16 => "0x",
                    2 => "0b",
                    8 => "0o",
                    _ => "",
                };
                self.write(&format!("{prefix}{digits}n"));
            }
            Expr::Bool(b) => self.write(if *b { "true" } else { "false" }),
            Expr::Null => self.write("null"),
            Expr::Uninit => {} // declared-but-uninit; handled by LetDecl
            Expr::String(s) => {
                self.write("'");
                for c in s.chars() {
                    match c {
                        '\\' => self.write("\\\\"),
                        '\'' => self.write("\\'"),
                        '\n' => self.write("\\n"),
                        '\t' => self.write("\\t"),
                        '\r' => self.write("\\r"),
                        c => self.out.push(c),
                    }
                }
                self.write("'");
            }
            Expr::Regex { pattern, flags } => {
                self.write("/");
                self.write(pattern);
                self.write("/");
                self.write(flags);
            }
            Expr::Member { obj, name } => {
                self.fmt_expr(*obj);
                self.write(".");
                self.write(name);
            }
            Expr::OptChain { obj, name } => {
                self.fmt_expr(*obj);
                self.write("?.");
                self.write(name);
            }
            Expr::Index { obj, index } => {
                self.fmt_expr(*obj);
                self.write("[");
                self.fmt_expr(*index);
                self.write("]");
            }
            Expr::Call { callee, args } => {
                self.fmt_expr(*callee);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(*a);
                }
                self.write(")");
            }
            Expr::Assign { target, value } => {
                self.fmt_expr(*target);
                self.write(" = ");
                self.fmt_expr(*value);
            }
            Expr::Array(items) => {
                self.write("[");
                for (i, a) in items.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(*a);
                }
                self.write("]");
            }
            Expr::Spread { expr } => {
                self.write("...");
                self.fmt_expr(*expr);
            }
            Expr::ObjectLit { fields } => {
                self.write("{ ");
                for (i, (n, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    // shorthand: `{ x }` if value is an ident with the same name
                    if let Expr::Ident(vn) = self.ast.get_expr(*v)
                        && vn == n
                    {
                        self.write(n);
                    } else {
                        self.write(n);
                        self.write(": ");
                        self.fmt_expr(*v);
                    }
                }
                self.write(" }");
            }
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                self.fmt_params(params);
                if let Some(ret) = return_type {
                    self.write(": ");
                    self.write(ret);
                }
                self.write(" => ");
                if body.len() == 1
                    && let Stmt::Return(Some(eid)) = &body[0]
                {
                    self.fmt_expr(*eid);
                } else {
                    self.write("{");
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
            }
            Expr::Closure { fn_name, captures } => {
                // Synthetic shape — only appears post-`lift_arrow_fns`.
                // Pre-desugar `format()` shouldn't see this. Print
                // recognizable but unparseable hint so users notice.
                self.write("/*closure ");
                self.write(fn_name);
                self.write(" captures=[");
                for (i, c) in captures.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(c);
                }
                self.write("]*/");
            }
            Expr::This => self.write("this"),
            Expr::New { class_name, args } => {
                self.write("new ");
                self.write(class_name);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(*a);
                }
                self.write(")");
            }
            Expr::Super { args } => {
                self.write("super(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(*a);
                }
                self.write(")");
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.fmt_expr(*cond);
                self.write(" ? ");
                self.fmt_expr(*then_branch);
                self.write(" : ");
                self.fmt_expr(*else_branch);
            }
            Expr::TypeOf { expr } => {
                self.write("typeof ");
                self.fmt_expr(*expr);
            }
            Expr::InstanceOf { expr, class_name } => {
                self.fmt_expr(*expr);
                self.write(" instanceof ");
                self.write(class_name);
            }
            Expr::Nullish { lhs, rhs } => {
                self.fmt_expr(*lhs);
                self.write(" ?? ");
                self.fmt_expr(*rhs);
            }
            Expr::PostIncr { target, is_inc } => {
                self.fmt_expr(*target);
                self.write(if *is_inc { "++" } else { "--" });
            }
            Expr::As { expr, ty_ann } => {
                self.fmt_expr(*expr);
                self.write(" as ");
                self.write(ty_ann);
            }
            Expr::Sequence { left, right } => {
                self.write("(");
                self.fmt_expr(*left);
                self.write(", ");
                self.fmt_expr(*right);
                self.write(")");
            }
            Expr::Unary { op, expr } => {
                let s = match op {
                    UnaryOp::Not => "!",
                    UnaryOp::Neg => "-",
                    UnaryOp::BitNot => "~",
                    UnaryOp::Plus => "+",
                };
                self.write(s);
                // Parenthesize complex operands defensively.
                let needs_paren = matches!(
                    self.ast.get_expr(*expr),
                    Expr::BinOp { .. } | Expr::Ternary { .. } | Expr::Assign { .. }
                );
                if needs_paren {
                    self.write("(");
                }
                self.fmt_expr(*expr);
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::BinOp { op, left, right } => {
                let needs_paren =
                    |child: ExprId| matches!(self.ast.get_expr(child), Expr::BinOp { .. });
                if needs_paren(*left) {
                    self.write("(");
                    self.fmt_expr(*left);
                    self.write(")");
                } else {
                    self.fmt_expr(*left);
                }
                self.write(" ");
                self.write(binop_str(op));
                self.write(" ");
                if needs_paren(*right) {
                    self.write("(");
                    self.fmt_expr(*right);
                    self.write(")");
                } else {
                    self.fmt_expr(*right);
                }
            }
        }
    }
}

fn binop_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::LAnd => "&&",
        BinOp::LOr => "||",
        BinOp::Eq => "===",
        BinOp::Neq => "!==",
        BinOp::LooseEq => "==",
        BinOp::LooseNeq => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::UShr => ">>>",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        format(src).expect("fmt")
    }

    #[test]
    fn simple_let() {
        let out = fmt("let x:i64=1\n");
        assert_eq!(out, "let x: i64 = 1\n");
    }

    #[test]
    fn fn_decl_and_call() {
        let src = "function add(a:i64,b:i64):i64{return a+b}\nadd(1,2)\n";
        let out = fmt(src);
        assert!(out.contains("function add(a: i64, b: i64): i64"));
        assert!(out.contains("return a + b"));
        assert!(out.contains("add(1, 2)"));
    }

    #[test]
    fn idempotent_round_trip() {
        let src = "let x:i64=1\nfunction id<T>(v:T):T{return v}\nid<i64>(x)\n";
        let one = fmt(src);
        let two = fmt(&one);
        assert_eq!(one, two, "fmt is not idempotent");
    }

    #[test]
    fn refuses_to_strip_comments() {
        let src = "// hello\nlet x: i64 = 1\n";
        assert!(matches!(format(src), Err(FormatError::CommentsPresent)));
    }

    #[test]
    fn block_comment_detected() {
        let src = "/* hello */\nlet x: i64 = 1\n";
        assert!(matches!(format(src), Err(FormatError::CommentsPresent)));
    }

    #[test]
    fn url_in_string_is_not_a_comment() {
        let src = "let url: string = 'https://example.com/path'\n";
        // Should NOT detect the `//` inside the string literal.
        let out = fmt(src);
        assert!(out.contains("https://example.com/path"));
    }

    #[test]
    fn single_quotes_emitted() {
        let src = "let s: string = \"hello\"\n";
        let out = fmt(src);
        assert!(out.contains("'hello'"));
    }
}
