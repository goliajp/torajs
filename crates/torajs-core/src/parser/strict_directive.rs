//! Per-function strictness (§11.2.2 "strict mode code") — the parser
//! bit that says the cursor sits inside a function whose directive
//! prologue said `"use strict"`, plus the normalization that writes
//! that INHERITED strictness back into each nested body as an
//! explicit directive of its own.
//!
//! ## Why materialize it instead of keying a side table
//!
//! A function body is the one carrier that survives the desugar
//! chain. `desugar_capturing_nested_fns` mints a fresh function
//! expression for a nested `function`, `lift_arrow_fns` renames every
//! closure, `desugar_nested_fns` lifts and renames again — every
//! ExprId and every name a parse-time table could key on is gone by
//! the time `fnexpr_this_faces::insert_sloppy_this_prologue` decides
//! whether a promoted body's detached `this` binds globalThis. The
//! body `Vec` itself is moved verbatim by all of them, so a directive
//! written at its head arrives intact — and the consumer needs no new
//! plumbing at all: it already probes the directive prologue
//! (rotation 375 knife 7).
//!
//! ## Why the formatter has to skip what this writes
//!
//! Not cosmetics: §15.1.3 makes an explicit `"use strict"` in a
//! function with a non-simple parameter list a SyntaxError, so a
//! formatter that re-emitted a synthetic directive could print source
//! its own parser rejects. The injected statements are therefore
//! recorded in `ast.synth_strict_directives`, and `tr fmt` skips
//! exactly those. The same early error is why injection stops at a
//! non-simple parameter list (see `finish_fn_body_strict`).
//!
//! ## Why the prologue is recognised at the STATEMENT level
//!
//! A token-level probe would have to re-derive ASI to tell the
//! directive `"use strict"` from the expression `"use strict" + x`,
//! and getting that subtly wrong binds `this` the wrong way round in
//! silence. Arming instead runs per parsed statement, comparing the
//! same cooked value the §15.1.3 gate compares (its precision note on
//! escaped spellings applies here too).

use super::*;

impl Parser<'_> {
    /// Called once per statement parsed into a function body, before
    /// the statement joins `seen`. While the body is still in its
    /// directive prologue — the leading run of string-literal
    /// expression statements — a `"use strict"` arms the bit for
    /// everything parsed afterwards, which is exactly the set of
    /// functions nested inside this one.
    pub(super) fn arm_strict_directive(&mut self, s: &Stmt, seen: &[Stmt]) {
        if self.in_strict_fn || self.directive_value(s) != Some("use strict") {
            return;
        }
        // Still inside the prologue only if nothing but string-literal
        // expression statements precedes this one.
        if seen.iter().all(|p| self.directive_value(p).is_some()) {
            self.in_strict_fn = true;
        }
    }

    /// §14.11.1 — a WithStatement is a SyntaxError in strict mode
    /// code. `with` reaches the parser as a plain identifier (the
    /// lexer's reserved table serves escaped spellings only), so the
    /// statement is recognised the way `interface` and `abstract` are
    /// in the dispatcher: the contextual name plus the one token that
    /// form must have next. Nothing is consumed — sloppy code keeps
    /// today's answer, and `with` being a ReservedWord in the grammar
    /// means no legal program can spell a call this way.
    pub(super) fn judge_with_statement(&mut self) -> Result<(), String> {
        let is_with = matches!(self.peek(), Token::Ident(s) if s == "with")
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|n| matches!(n.token, Token::LParen));
        if !is_with {
            return Ok(());
        }
        if self.in_strict_fn {
            return Err(format!(
                "`with` is not allowed in strict code at {} (ES §14.11.1)",
                self.at()
            ));
        }
        self.record_strict_goal_site("with");
        Ok(())
    }

    /// The program-level directive prologue — §11.2.2 makes global
    /// code strict when it opens with one, exactly as a function body
    /// is. Same recognition, but "still inside the prologue" is the
    /// caller's flag rather than a scan of what came before: the top
    /// level pushes straight into `ast.stmts`, which under `parse_into`
    /// may already hold an earlier module's statements, so the leading
    /// run of directives is not recoverable from that vector.
    pub(super) fn arm_strict_directive_program(&mut self, s: &Stmt, in_prologue: &mut bool) {
        if !*in_prologue {
            return;
        }
        match self.directive_value(s) {
            Some(v) => {
                if v == "use strict" {
                    self.in_strict_fn = true;
                }
            }
            None => *in_prologue = false,
        }
    }

    /// The cooked value of `s` when it is an expression statement
    /// holding a string literal — i.e. a directive, as far as the
    /// prologue grammar is concerned.
    fn directive_value(&self, s: &Stmt) -> Option<&str> {
        let Stmt::Expr(id) = s else { return None };
        match self.ast.exprs.get(id.0 as usize) {
            Some(Expr::String(v)) => Some(v.as_str()),
            _ => None,
        }
    }

    /// Pop back to the enclosing function's strictness, for the body
    /// sites that only need the bit scoped — arrows, whose `this` is
    /// lexical, and generator bodies, whose receiver comes from the
    /// state-machine class rather than a promoted prologue. Judges the
    /// parameter names on the way out, for the reason
    /// `finish_fn_body_strict` gives.
    pub(super) fn restore_fn_strict(
        &mut self,
        inherited: bool,
        params: &[Param],
    ) -> Result<(), String> {
        let strict = self.in_strict_fn;
        self.in_strict_fn = inherited;
        self.reject_strict_reserved_params(params, strict)
    }

    /// §12.7.2 and §13.1.1 in a parameter list. This runs at the END
    /// of the function-body parse, not where the names were read,
    /// because a function's own `"use strict"` sits INSIDE the body it
    /// precedes: at parameter-parse time the directive has not been
    /// seen yet, so `function f(static) { "use strict" }` would slip
    /// through a check placed where the name is consumed.
    ///
    /// The sloppy branch still has to park the names: a parameter is a
    /// binding the goal gate would otherwise never hear about, since
    /// nothing else re-reads a `Param` name. The recorded position is
    /// the end of the body rather than the parameter itself — the same
    /// deferral that makes this check correct costs the exact offset,
    /// and the message names the word.
    fn reject_strict_reserved_params(
        &mut self,
        params: &[Param],
        strict: bool,
    ) -> Result<(), String> {
        for p in params {
            if strict {
                self.reject_if_strict_binding(&p.name, true)?;
            } else if super::strict_reserved::refused_as_binding(&p.name) {
                self.record_strict_goal_site(&p.name);
            }
        }
        Ok(())
    }

    /// Restore the enclosing function's strictness bit and, when this
    /// body is strict only by INHERITANCE, give it a directive of its
    /// own so the rest of the pipeline can read the fact off the body.
    ///
    /// Call site is uniform: immediately after
    /// `reject_use_strict_with_non_simple_params`, on the raw body,
    /// before any per-parameter destructuring `let`s are prepended.
    /// That ordering is load-bearing twice over — the §15.1.3 gate has
    /// already judged the body the user actually wrote, so a synthetic
    /// directive can never manufacture that SyntaxError, and the
    /// directive lands at index 0 of a body no prefix has been added
    /// to yet, which is where a prologue probe looks.
    ///
    /// A non-simple parameter list is skipped rather than injected
    /// into: that is the very shape §15.1.3 forbids from carrying the
    /// directive, and the prepended destructuring `let`s would push it
    /// off the head anyway. Such a function keeps today's binding —
    /// recorded residue, not a silent wrap.
    pub(super) fn finish_fn_body_strict(
        &mut self,
        inherited: bool,
        params: &[Param],
        body: &mut Vec<Stmt>,
    ) -> Result<(), String> {
        let strict = self.in_strict_fn;
        self.in_strict_fn = inherited;
        self.reject_strict_reserved_params(params, strict)?;
        if !inherited {
            // Sloppy here, or strict by this body's own directive —
            // which is what the probe downstream already reads.
            return Ok(());
        }
        // A prologue that already says it needs nothing written.
        if body
            .iter()
            .map_while(|s| self.directive_value(s))
            .any(|v| v == "use strict")
        {
            return Ok(());
        }
        let simple = params
            .iter()
            .all(|p| p.default.is_none() && !p.is_rest && !p.name.starts_with("__param_destr_"));
        if !simple {
            return Ok(());
        }
        let e = self.ast.add_expr(Expr::String("use strict".to_string()));
        self.ast.synth_strict_directives.insert(e);
        body.insert(0, Stmt::Expr(e));
        Ok(())
    }
}
