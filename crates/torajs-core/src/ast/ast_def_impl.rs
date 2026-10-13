//! Methods on [`Ast`]; the struct itself is in `ast_def.rs`.

use super::*;

impl Ast {
    /// Whether this program's code is SLOPPY — the question nearly
    /// every reader of `sloppy_script_goal` actually means.
    ///
    /// Two bits, because a Script can be strict without changing goal:
    /// `"use strict";` at the top of a `.cts` is strict Script code
    /// (§11.2.2), and every rule phrased "in strict code" then applies
    /// to it exactly as it does to a module — mapped `arguments` and
    /// `callee` (§10.4.4), `delete <name>` (§13.5.1.1), `with`
    /// (§13.11.1), duplicate parameters (§15.1.2), the §12.7.2
    /// reserved words, legacy octal (§12.9.3), Annex B, implicit
    /// globals, and the undefined-receiver `this` substitution
    /// (§10.2.1.2).
    ///
    /// Asking the goal alone is what made `"use strict"; function f(a)
    /// { a = 1; return arguments[0]; }` hand back a MAPPED arguments
    /// object in a Script — and 74 test262 cases were passing only
    /// because staging them as modules had been supplying the
    /// strictness their own prologue was meant to.
    ///
    /// Per-FUNCTION strictness is a different question, answered where
    /// it is known (`Parser::in_strict_fn`, during the parse). This one
    /// is about the program.
    pub fn program_is_sloppy(&self) -> bool {
        self.sloppy_script_goal && !self.program_strict_prologue
    }

    /// Whether `body` — one function's body — is sloppy. Strictness has
    /// a second source below the program: §11.2.2 makes a function with
    /// a `"use strict"` directive prologue strict even inside a sloppy
    /// Script, and that is what decides the per-function shapes
    /// (mapped vs unmapped `arguments`, and whether `callee` is a data
    /// property or the `%ThrowTypeError%` accessor).
    ///
    /// Inherited strictness needs no propagation: the parser
    /// materializes a directive into every strict-by-inheritance body
    /// (`parser::strict_directive`), so reading this body is enough.
    ///
    /// Reach for this, not [`Self::program_is_sloppy`], whenever the
    /// answer describes a function rather than the program.
    pub fn body_is_sloppy(&self, body: &[Stmt]) -> bool {
        self.program_is_sloppy() && !self.body_says_use_strict(body)
    }

    /// Whether `body`'s own directive prologue holds a Use Strict
    /// Directive (see `sloppy_this_prologue::has_use_strict_directive`).
    pub fn body_says_use_strict(&self, body: &[Stmt]) -> bool {
        crate::ast::sloppy_this_prologue::has_use_strict_directive(
            body,
            &self.exprs,
            &self.escaped_use_strict,
        )
    }

    /// RFC 20260921 — a fresh `Stmt::ClassDecl::decl_id` (see there).
    /// Ids start at 1, leaving 0 the "no declaration of its own"
    /// sentinel; every construction point mints, `BodyCloner` too.
    pub fn mint_class_decl_id(&mut self) -> u32 {
        self.next_class_decl_id += 1;
        self.next_class_decl_id
    }

    pub fn add_expr(&mut self, e: Expr) -> ExprId {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(e);
        // Sentinel span until parser (or a desugar pass) sets a real
        // one via `set_expr_span`. Both fields zero means "unknown".
        self.expr_spans
            .push(crate::lexer::Span { start: 0, end: 0 });
        id
    }

    /// v0.3 #4 — record the source byte range of `eid`'s originating
    /// token (or sub-token range). Idempotent in the sense that
    /// later calls overwrite earlier ones; parser is the canonical
    /// caller, but desugar passes that emit synthetic Exprs may also
    /// inherit a span from their originating user node.
    pub fn set_expr_span(&mut self, eid: ExprId, span: crate::lexer::Span) {
        if (eid.0 as usize) < self.expr_spans.len() {
            self.expr_spans[eid.0 as usize] = span;
        }
    }

    /// Build the newline offset table once. Idempotent. Call this
    /// after setting `self.source`; subsequent `byte_to_line_col`
    /// lookups become `&self`-only and can be invoked from
    /// borrow-restricted contexts (codegen, etc).
    pub fn warm_newline_cache(&mut self) {
        if !self.newline_offsets.is_empty() || self.source.is_empty() {
            return;
        }
        for (i, b) in self.source.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                self.newline_offsets.push(i as u32);
            }
        }
    }

    /// Translate a source byte offset into a (line, col) pair, both
    /// 1-indexed (DWARF / editor convention). Returns (0, 0) if the
    /// offset is past end-of-source (sentinel for "no location").
    /// Requires `warm_newline_cache` to have been called when source
    /// is non-empty; otherwise returns (0, 0) for non-zero offsets.
    pub fn byte_to_line_col(&self, byte: u32) -> (u32, u32) {
        if byte == 0 || (byte as usize) > self.source.len() {
            return (0, 0);
        }
        let nl = &self.newline_offsets;
        if nl.is_empty() && !self.source.is_empty() {
            // Cache not warmed; line 1, col=byte+1 as fallback.
            return (1, byte + 1);
        }
        let line = match nl.binary_search(&byte) {
            Ok(k) => (k as u32) + 1,
            Err(k) => (k as u32) + 1,
        };
        let line_start = if line == 1 {
            0u32
        } else {
            nl[(line - 2) as usize] + 1
        };
        let col = byte - line_start + 1;
        (line, col)
    }

    /// Record a generator declaration's destructuring-param prefix, by
    /// name and by the declaration itself.
    pub(crate) fn note_gen_destr_prefix(&mut self, name: &str, span: crate::lexer::Span, n: usize) {
        self.gen_param_destr_prefix.insert(name.to_string(), n);
        self.gen_param_destr_prefix_at
            .insert((name.to_string(), span.start, span.end), n);
    }

    pub fn get_expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.0 as usize]
    }

    pub fn print(&self) {
        for s in &self.stmts {
            crate::ast_printer::print_stmt(self, s, 0);
        }
    }
}
