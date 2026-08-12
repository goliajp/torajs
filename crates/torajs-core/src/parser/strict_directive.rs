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

    /// §12.7.2 in a parameter list. This runs at the END of the
    /// function-body parse, not where the names were read, because a
    /// function's own `"use strict"` sits INSIDE the body it precedes:
    /// at parameter-parse time the directive has not been seen yet, so
    /// `function f(static) { "use strict" }` would slip through a
    /// check placed where the name is consumed.
    fn reject_strict_reserved_params(&self, params: &[Param], strict: bool) -> Result<(), String> {
        if !strict {
            // Sloppy: ordinary identifiers. The goal half is not
            // recorded here — a parameter list is re-read from the
            // `Param` names by nothing else, and the strict GOAL makes
            // the whole file strict, which the declaration-site
            // recording already covers for every binding the file
            // introduces.
            return Ok(());
        }
        for p in params {
            self.reject_if_strict_reserved(&p.name, true)?;
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
