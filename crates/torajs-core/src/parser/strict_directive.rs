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
            self.record_strict_prologue_body();
        }
    }

    /// Record the byte range this `"use strict"` just made strict, for
    /// the gates that can only judge after the parse — today the
    /// Annex B legacy-octal one ([`crate::ast::legacy_octal_sites`]).
    ///
    /// The range has to be the WHOLE body, not the part after the
    /// directive: §11.2.2 makes the entire function strict code, and
    /// §12.9.4.1 is asked of the literals in the prologue itself.
    /// `function f() { "\1"; "use strict"; }` is a SyntaxError, and the
    /// offending literal sits BEFORE the directive that condemns it.
    ///
    /// Recovered from the token stream rather than threaded through the
    /// eight body-parsing sites that arm this bit. Each of them would
    /// have had to remember its own opening brace, and eight remembering
    /// sites is eight chances for one of them to forget; the brace is
    /// already in the stream, at the one place a scan can find it — the
    /// nearest unclosed `{` before the cursor. Template `${…}` braces
    /// live inside their own `Token::Template` and never reach this
    /// stream, so nothing else can be mistaken for it.
    fn record_strict_prologue_body(&mut self) {
        let span = self
            .enclosing_brace_span()
            .unwrap_or((0, self.source.len() as u32));
        self.ast.strict_prologue_spans.push(span);
    }

    /// `(start, end)` bytes of the innermost brace pair enclosing the
    /// cursor, or `None` at the top level (program code, whose prologue
    /// makes the whole source strict).
    fn enclosing_brace_span(&self) -> Option<(u32, u32)> {
        let mut depth = 0i32;
        let open = (0..self.pos).rev().find(|&i| {
            match self.tokens[i].token {
                Token::RBrace => depth += 1,
                Token::LBrace => {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            false
        })?;
        depth = 0;
        let close = (open + 1..self.tokens.len()).find(|&i| {
            match self.tokens[i].token {
                Token::LBrace => depth += 1,
                Token::RBrace => {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            false
        })?;
        Some((self.tokens[open].span.start, self.tokens[close].span.end))
    }

    /// §14.11.1 — a WithStatement is a SyntaxError in strict mode
    /// code. `with` reaches the parser as a plain identifier (the
    /// lexer's reserved table serves escaped spellings only), so the
    /// statement is recognised the way `interface` and `abstract` are
    /// in the dispatcher: the contextual name plus the one token that
    /// form must have next. Nothing is consumed — sloppy code keeps
    /// today's answer, and `with` being a ReservedWord in the grammar
    /// means no legal program can spell a call this way.
    ///
    /// Strictness has a source here that the per-function bit does not
    /// carry: §11.2.2 makes every part of a class strict code whatever
    /// the goal and whatever the enclosing function said, so
    /// `class_stack` counts alongside it — the same third source
    /// `note_strict_reference` and the `yield` admission sites already
    /// read. Without it `class K { m() { with (o) { x } } }` parsed,
    /// and then nothing downstream owned the shape either: the marker
    /// block sits in a method body, which no walk of this desugar
    /// reaches, so `x` resolved lexically and the program died on an
    /// `unknown identifier` at run time. A wrong answer where the
    /// spec's answer is a SyntaxError — and a refusal a stderr-
    /// classifying harness reads as tr having RUN the program.
    pub(super) fn judge_with_statement(&mut self) -> Result<(), String> {
        let is_with = matches!(self.peek(), Token::Ident(s) if s == "with")
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|n| matches!(n.token, Token::LParen));
        if !is_with {
            return Ok(());
        }
        if self.strict_here() {
            return Err(format!(
                "`with` is not allowed in strict code at {} (ES §14.11.1)",
                self.at()
            ));
        }
        self.record_strict_goal_site("with");
        self.reject_with_body_declaration()
    }

    /// §14.11 — `with ( Expression ) Statement`, and the Statement
    /// production excludes every Declaration. Sloppy code is the only
    /// caller that gets here (strict already refused above), and it
    /// would otherwise read `with ({}) function f() {}` as a call
    /// followed by an ordinary function declaration.
    fn reject_with_body_declaration(&self) -> Result<(), String> {
        let Some(mut at) = self.after_matching_rparen(self.pos + 1) else {
            // Unbalanced — the ordinary parse reports it better than a
            // guess about the body would.
            return Ok(());
        };
        // A LabelledStatement is a Statement, but §14.13.1 refuses a
        // function as its body wherever a plain function declaration is
        // refused, so the label chain is walked through.
        while matches!(self.tokens.get(at).map(|s| &s.token), Some(Token::Ident(_)))
            && matches!(
                self.tokens.get(at + 1).map(|s| &s.token),
                Some(Token::Colon)
            )
        {
            at += 2;
        }
        let Some(t) = self.tokens.get(at).map(|s| &s.token) else {
            return Ok(());
        };
        let next = self.tokens.get(at + 1).map(|s| &s.token);
        let kind = match t {
            Token::Function => "a function declaration",
            Token::Class => "a class declaration",
            Token::Const => "a `const` declaration",
            // `let` only opens a declaration when a binding follows
            // it, and §13.16's lookahead restriction forbids only
            // `let [` — so `let` then a LINE BREAK then an identifier
            // is an ASI-split pair of expression statements, which IS
            // a Statement and stays legal. Same exemption, same
            // helper, as the loop-body rule (`let_newline_asi_form`,
            // named for the very test262 family that pins it).
            Token::Let
                if matches!(next, Some(Token::LBracket))
                    || (matches!(next, Some(Token::Ident(_)) | Some(Token::LBrace))
                        && !self.let_newline_asi_form(at)) =>
            {
                "a `let` declaration"
            }
            Token::Async if matches!(next, Some(Token::Function)) => {
                "an async function declaration"
            }
            _ => return Ok(()),
        };
        Err(format!(
            "the body of a `with` statement cannot be {kind} at {} (ES §14.11)",
            self.at()
        ))
    }

    /// Index of the token after the `)` that closes the `(` at `open`,
    /// or `None` when the parentheses do not balance before end of
    /// input.
    fn after_matching_rparen(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (i, s) in self.tokens.iter().enumerate().skip(open) {
            match s.token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                Token::Eof => return None,
                _ => {}
            }
        }
        None
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
                    self.ast.program_strict_prologue = true;
                    self.record_strict_prologue_body();
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
            Some(Expr::String(v)) => v.as_str(),
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
        self.reject_strict_reserved_params(params, strict)?;
        self.judge_duplicate_params_strict(params, strict)
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

    /// §15.1.2 — duplicate BoundNames in a FormalParameters list are
    /// a SyntaxError when that list is strict mode code. The
    /// goal-independent halves (UniqueFormalParameters, and any
    /// non-simple list) belong to `reject_duplicate_params` and are
    /// already refused where the names are read; this is the half
    /// that has to wait, and it waits here for exactly the reason
    /// `reject_strict_reserved_params` above does —
    /// `function f(a, a) { "use strict"; }` puts the directive inside
    /// the body its parameters precede.
    ///
    /// `strict` is the function's OWN bit, passed in because the
    /// caller has already restored the enclosing one. The class
    /// source is not in it (`class_stack` is what
    /// [`Parser::strict_here`] adds), and it is still live here — the
    /// method whose body just closed is inside its class.
    ///
    /// The sloppy branch parks the site: under a strict goal every
    /// FormalParameters in the file is strict code, and the goal is
    /// only stamped after the parse. Sloppy code with a simple list
    /// keeps `function f(a, a) {}` legal, which test262 asserts
    /// (`param-duplicated-non-strict.js`, `S10.2.1_A2.js`).
    fn judge_duplicate_params_strict(
        &mut self,
        params: &[Param],
        strict: bool,
    ) -> Result<(), String> {
        let Some(dup) = params
            .iter()
            .enumerate()
            .find(|(i, p)| params[..*i].iter().any(|q| q.name == p.name))
            .map(|(_, p)| p.name.clone())
        else {
            return Ok(());
        };
        let at = self.at();
        if strict || !self.class_stack.is_empty() {
            return Err(format!(
                "duplicate parameter name `{dup}` is not allowed in strict code \
                 at {at} (ES §15.1.2)"
            ));
        }
        self.ast.dup_param_positions.push((at, dup));
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
        self.judge_duplicate_params_strict(params, strict)?;
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
        let e = self
            .ast
            .add_expr(Expr::String("use strict".to_string().into()));
        self.ast.synth_strict_directives.insert(e);
        body.insert(0, Stmt::Expr(e));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// The parse verdict for `src`: `Ok` or the error text.
    fn verdict(src: &str) -> Result<(), String> {
        let tokens = tokenize(src).expect("tokenize");
        parse(src, &tokens).map(|_| ())
    }

    /// §11.2.2 — every part of a class is strict code, so §14.11.1
    /// applies inside one even when the goal is sloppy. Each member
    /// position is listed on its own: they are parsed by different
    /// entry points, and the shared gate has to be reached from all of
    /// them.
    #[test]
    fn with_inside_a_class_body_is_a_syntax_error() {
        for src in [
            "var o = {};\nclass K { m() { with (o) { } } }\n",
            "var o = {};\nclass K { constructor() { with (o) { } } }\n",
            "var o = {};\nclass K { static s() { with (o) { } } }\n",
            "var o = {};\nclass K { static { with (o) { } } }\n",
            "var o = {};\nconst C = class { m() { with (o) { } } };\n",
            // Strictness is inherited by anything nested in the body,
            // so an ordinary function written there is strict too.
            "var o = {};\nclass K { m() { function f() { with (o) { } } } }\n",
        ] {
            let err = verdict(src).expect_err(src);
            assert!(
                err.contains("§14.11.1"),
                "expected the §14.11.1 refusal for {src:?}, got {err}"
            );
        }
    }

    /// The control: a sloppy `with` outside any class still parses.
    /// Without it the assertion above passes just as well when the
    /// gate refuses every `with` there is.
    #[test]
    fn with_outside_a_class_still_parses_in_sloppy_code() {
        verdict("var o = {};\nwith (o) { }\n").expect("sloppy with");
    }
}
