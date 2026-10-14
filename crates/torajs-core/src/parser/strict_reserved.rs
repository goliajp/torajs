//! The names strict mode code refuses, and where it refuses them.
//!
//! Two lists, because they are refused in different positions:
//!
//! * **§12.7.2 future reserved words** — `implements interface package
//!   private protected public static`. Reserved in EVERY `Identifier`
//!   occurrence of strict code, reference positions included. (`let`
//!   and `yield` belong to the same list but reach the parser as their
//!   own tokens; `yield` keeps the dedicated `yield_ident_positions`
//!   lane it got in rotation 374.)
//! * **§13.1.1 `eval` / `arguments`** — refused only where strict code
//!   would BIND or ASSIGN them. Reading them stays legal everywhere:
//!   `typeof eval` is fine in the strictest module there is, which is
//!   why this half must never be folded into the reference judge.
//!
//! Two answers, because strictness arrives from two directions:
//!
//! * **Per-function** — the enclosing function said `"use strict"`,
//!   possibly several levels up (`parser::strict_directive`). The
//!   parser knows this while it parses, so the SyntaxError is raised
//!   on the spot.
//! * **Per-goal** — module code is strict (§16.1), but the goal bit is
//!   stamped AFTER parsing. So the site is admitted and recorded, and
//!   the prelude gate `triage_strict_reserved_idents` raises it, the
//!   same shape `yield` and `delete <bare name>` already use.
//!
//! Sloppy script code keeps every one of these as an ordinary
//! identifier, which is why the default has to be admission.

use super::reserved_ident::is_ident_spelled_reserved;
use super::*;

/// The strict-only future reserved words that arrive as plain
/// identifiers.
const STRICT_RESERVED: [&str; 7] = [
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
    "static",
];

/// §13.1.1 — refused in binding and assignment positions of strict
/// code, admitted everywhere else.
const STRICT_BAD_BINDING: [&str; 2] = ["eval", "arguments"];

/// Whether `name` is refused where strict code binds or assigns it —
/// the union of both lists, since a future reserved word is refused in
/// every position including these.
///
/// `let` is on the list too: the literal word reaches the parser as its
/// own token, so a `"let"` name here was spelled with an escape
/// (`var l\u0065t`), which §12.7.2 judges on its StringValue like the
/// bare word.
pub(super) fn refused_as_binding(name: &str) -> bool {
    STRICT_RESERVED.contains(&name) || STRICT_BAD_BINDING.contains(&name) || name == "let"
}

/// The §12.7.2 list as a REFERENCE position sees it, which is the
/// seven words plus `let`.
///
/// `let` is on the same clause of the spec but reaches the parser as
/// its own token, so every other occurrence is judged at the site by
/// `let_reads_as_ident`. One position judges by NAME instead: the
/// object-literal shorthand. There the property-name reader has
/// already accepted the token and turned it into a string, and the
/// shorthand shape then re-reads that string as an
/// IdentifierReference — so `{ let }` has to be refused in strict
/// code while `{ let: 1 }` stays legal, and this is where that is
/// decided.
fn refused_as_reference(name: &str) -> bool {
    STRICT_RESERVED.contains(&name) || name == "let"
}

impl Parser<'_> {
    /// Judge `name` in a BindingIdentifier position. `Err` when the
    /// position is already known to be strict; otherwise the site is
    /// recorded for the goal gate and `Ok` admits it.
    ///
    /// `strict` is passed in rather than read off `self` because the
    /// two differ at the sites that must defer: a function's own
    /// `"use strict"` lives inside the body, so its NAME and its
    /// PARAMETERS are only judgeable once that body has been parsed —
    /// by which point `self.in_strict_fn` has been restored to the
    /// enclosing function's answer.
    pub(super) fn note_strict_binding(&mut self, name: &str, strict: bool) -> Result<(), String> {
        self.refuse_ident_spelled_reserved(name)?;
        if !refused_as_binding(name) {
            return Ok(());
        }
        self.reject_if_strict_binding(name, strict)?;
        self.record_strict_goal_site(name);
        Ok(())
    }

    /// Park `name` for the goal gate, for callers that have already
    /// answered the per-function half themselves.
    pub(super) fn record_strict_goal_site(&mut self, name: &str) {
        let at = self.at();
        self.ast
            .strict_reserved_positions
            .push((at, name.to_string()));
    }

    /// Binding-position judge, with the verdict passed IN rather than
    /// read off `self`. The parameter-list caller has already restored
    /// the enclosing function's bit by the time it asks — its own
    /// strictness came from the body that has just finished parsing —
    /// so consulting `self.in_strict_fn` here would answer for the
    /// wrong function.
    pub(super) fn reject_if_strict_binding(&self, name: &str, strict: bool) -> Result<(), String> {
        self.refuse_ident_spelled_reserved(name)?;
        if strict && STRICT_BAD_BINDING.contains(&name) {
            return Err(format!(
                "`{name}` cannot be bound in strict code at {} (ES §13.1.1)",
                self.at()
            ));
        }
        self.reject_if_strict_reserved(name, strict)
    }

    /// Whether the cursor sits in strict mode code, as far as the
    /// PARSER can tell. Two of §11.2.2's three sources are visible
    /// here: an explicit `"use strict"` prologue, on this function or
    /// on the program, which `in_strict_fn` carries; and a class body,
    /// which is strict whatever the goal said and whatever encloses
    /// it. The third — the goal itself, a module being strict code —
    /// is only stamped after the parse, so every judgement built on
    /// this predicate needs a goal-half gate to go with it.
    ///
    /// One spelling, because there were four: `judge_with_statement`
    /// and `note_strict_reference` wrote the `||` form,
    /// `let_reads_as_ident` and `yield_reads_as_ident` wrote its
    /// negation, and the Annex B function-declaration site below was
    /// about to write a fifth.
    pub(super) fn strict_here(&self) -> bool {
        self.strict_or_in_class(self.in_strict_fn)
    }

    /// §15.7.1 — all parts of a class are strict mode code, whatever
    /// the goal says and whatever the enclosing function said.
    ///
    /// For callers holding a function's OWN strictness rather than the
    /// parser's current bit: a directive prologue lives inside the body,
    /// so a function's name and parameters are only judgeable once that
    /// body is parsed, by which point `in_strict_fn` has been restored
    /// to the enclosing answer. Those callers have the right first
    /// operand already and were missing only this clause — which is why
    /// `class C { m(eval) {} }` was accepted under the Script goal while
    /// `"use strict"; function f(eval) {}` was not.
    /// The spelling of a token that names a valid BindingIdentifier
    /// but does not arrive as `Token::Ident` — a CONTEXTUAL keyword.
    ///
    /// `async` and `type` are on nobody's reserved list: `async` is a
    /// keyword only where the grammar looks ahead for it (`async
    /// function`, `async (x) =>`), and `type` is TypeScript's, in
    /// declaration position only. `var async = 1` and `{ async }` are
    /// ordinary programs — node runs both.
    ///
    /// `let`, `yield` and `await` also name bindings in the right
    /// positions, but each carries a judgement of its own
    /// (`let_reads_as_ident` and its two siblings) and `await`'s
    /// admitted sites have to be recorded for the module-goal gate, so
    /// they are not folded in here.
    pub(super) fn contextual_binding_name(t: &Token) -> Option<&'static str> {
        match t {
            Token::Async => Some("async"),
            Token::Type => Some("type"),
            _ => None,
        }
    }

    /// The spelling a token binds under in a destructuring SHORTHAND
    /// position (`var { x } = o`), or `None` when the position needs
    /// the explicit `field: binding` rename.
    ///
    /// Four words arrive here that are not `Token::Ident`. `async` and
    /// `type` are unconditional — nobody reserves them. The other two
    /// each carry the judgement that governs that word and nothing
    /// else, which is why they are not folded into
    /// [`Parser::contextual_binding_name`]:
    ///
    /// - `yield` — reserved inside a generator, which spells a
    ///   YieldExpression instead, and in strict code (§12.7.2).
    ///   Sloppy non-generator code binds it, in every shorthand
    ///   position measured: a parameter pattern, a catch parameter, an
    ///   arrow's parameter, and `for (let { yield } of …)` — §14.3.1.1
    ///   restricts `let` in the BoundNames of a lexical declaration,
    ///   and says nothing about `yield`.
    /// - `await` — reserved by the grammar's `[Await]` parameter alone
    ///   (§15.8.1). The Module-goal half of the question is stamped
    ///   after the parse, so an admitted site is RECORDED for
    ///   `triage_await_idents` rather than answered here. That
    ///   recording is the whole reason this takes `&mut self`.
    ///
    /// - `let` — §12.7.2 like `yield`, PLUS §14.3.1.1, which refuses it
    ///   in the BoundNames of a lexical declaration in sloppy code too.
    ///   That second half is `pattern_binds_lexically`, and the axis it
    ///   draws is lexical-vs-var, NOT var-vs-everything. Measured
    ///   against node in a sloppy script, eleven cells, tr matching all
    ///   eleven: `var { let }`, a function parameter, an arrow
    ///   parameter, `for (var { let } of …)`, a nested pattern inside
    ///   any of them, and the `{ let: x }` rename all bind;
    ///   `let { let }`, `const { let }`, `for (let { let } of …)`,
    ///   `"use strict"` and — the cell that settles the axis —
    ///   **`catch ({ let })`** all reject, while `catch ({ yield })`
    ///   binds.
    ///   The reserved word a token SPELLS — bare, or written with a
    ///   unicode escape. §12.7.2 makes the escaped form never the
    ///   keyword, but a BindingIdentifier is judged on its StringValue,
    ///   so `\u0079ield` may name something exactly where `yield` may
    ///   and `\u0069f` may nowhere. Every admission site asks through
    ///   here so the two spellings cannot drift apart.
    pub(super) fn reserved_word_of(t: &Token) -> Option<&str> {
        match t {
            Token::Let => Some("let"),
            Token::Yield => Some("yield"),
            Token::Await => Some("await"),
            Token::EscapedIdent(n) => Some(n.as_str()),
            _ => None,
        }
    }

    /// Does `t` spell `yield` — bare or escaped — where this position
    /// reads it as an ordinary name? The two halves always travel
    /// together, so every admission site asks one question.
    pub(super) fn yield_names_here(&self, t: &Token) -> bool {
        Self::reserved_word_of(t) == Some("yield") && self.yield_reads_as_ident()
    }

    /// The `await` twin of [`Self::yield_names_here`].
    pub(super) fn await_names_here(&self, t: &Token) -> bool {
        Self::reserved_word_of(t) == Some("await") && self.await_reads_as_ident()
    }

    pub(super) fn shorthand_binding_name(&mut self) -> Option<&'static str> {
        match self.peek() {
            // `let` needs the binding form as well as the strictness:
            // §14.3.1.1 refuses it in the BoundNames of a lexical
            // declaration even in sloppy code.
            t if Self::reserved_word_of(t) == Some("let")
                && self.let_reads_as_ident()
                && !self.pattern_binds_lexically =>
            {
                // Admitted sites park for the goal gate, same as every
                // other `let` / `yield` admission: `let_reads_as_ident`
                // and `yield_reads_as_ident` answer the PER-FUNCTION
                // half of §12.7.2, and only `ast.program_is_sloppy()`
                // — stamped after the parse — answers the goal half.
                // Without the record, `var { let } = {}` and every
                // pattern slot beside it bound in a MODULE, where node
                // reports "Unexpected strict mode reserved word".
                self.record_strict_goal_site("let");
                Some("let")
            }
            t if self.yield_names_here(t) => {
                let at = self.at();
                self.ast.yield_ident_positions.push(at);
                Some("yield")
            }
            t if self.await_names_here(t) => {
                self.admit_await_ident();
                Some("await")
            }
            // Re-peeked rather than bound: an arm above needs `&mut
            // self`, so no arm may hold a reference from the scrutinee.
            _ => Self::contextual_binding_name(self.peek()),
        }
    }

    /// The name a BINDING POSITION reads here: an ordinary
    /// `Ident`, or a contextual keyword this position admits as one.
    /// Peek only — the caller advances.
    ///
    /// Every slot in a destructuring pattern is such a position, and
    /// they used to disagree: the object half asked
    /// [`Self::shorthand_binding_name`] at its shorthand slot while
    /// the array elements, both rest slots and the rename targets
    /// matched `Token::Ident` alone, so `var { let }` bound and
    /// `var [ let ]`, `var { ...let }` and `var { a: let }` were
    /// SyntaxErrors that node runs. One spelling, asked everywhere.
    pub(super) fn pattern_binding_name(&mut self) -> Option<String> {
        if let Token::Ident(n) = self.peek() {
            if is_ident_spelled_reserved(n) {
                return None;
            }
            return Some(n.clone());
        }
        self.shorthand_binding_name().map(|s| s.to_string())
    }

    /// Run `f` with `pattern_binds_lexically` cleared — for a
    /// position that NAMES something without lexically declaring it.
    /// §14.3.1.1 restricts `let` in the BoundNames of a `let` /
    /// `const` declaration and says nothing about anywhere else, so a
    /// label, a `var` binding, a formal parameter and a catch
    /// parameter all admit the word that `let { let }` refuses.
    pub(super) fn in_non_lexical_position<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = std::mem::replace(&mut self.pattern_binds_lexically, false);
        let r = f(self);
        self.pattern_binds_lexically = saved;
        r
    }

    pub(super) fn strict_or_in_class(&self, own: bool) -> bool {
        own || !self.class_stack.is_empty() || self.in_class_heritage
    }

    /// Whether a bare `yield` here may be read as an ordinary
    /// identifier — the question seven of the eight admission sites
    /// ask, and used to ask two thirds of each. (The eighth,
    /// `fn_expr`'s function-expression name, spells its clauses out
    /// for the §15.2 reason recorded there.)
    ///
    /// Three things reserve it, and only the third is new: a generator
    /// body spells a YieldExpression instead; a class body is strict
    /// by §15.7 whatever the goal says; and §12.7.2 reserves it in
    /// strict code generally, which since rotation 376 the parser can
    /// finally answer per function rather than leaving all of it to
    /// the strict-goal gate. A site that declines here falls through
    /// to its own "expected an identifier" reject, which is the
    /// SyntaxError the spec wants.
    ///
    /// Sloppy code outside a generator keeps `yield` as a name, and
    /// the admitted site is still recorded for the goal gate.
    pub(super) fn yield_reads_as_ident(&self) -> bool {
        !self.in_generator && !self.strict_here()
    }
    /// Whether a bare `let` here may be read as an ordinary
    /// identifier — the `yield` predicate's sibling, and the last of
    /// §12.7.2's list to reach the parser as its own token.
    ///
    /// The direction is the OPPOSITE of `yield`'s. `yield` arrived
    /// admitted everywhere and needed the strict half added; `let`
    /// arrived refused everywhere — even in a sloppy script, where
    /// `var let = 1` is an ordinary declaration bun answers `1` for.
    /// So what this predicate opens is the sloppy half, and the two
    /// clauses that close it are the ones `yield` shares: a class
    /// body is strict by §15.7 whatever the goal says, and §12.7.2
    /// reserves the word in strict code, which the parser answers per
    /// function since rotation 376. There is no generator clause —
    /// `let` means the same thing inside one and outside.
    ///
    /// A site that declines falls through to its own "expected an
    /// identifier" reject, which is the SyntaxError the spec wants.
    /// An admitted site records itself on the shared
    /// `strict_reserved_positions` vector, whose §12.7.2 message is
    /// already the right one for this word — `yield` needed a lane of
    /// its own for reasons that do not apply here.
    ///
    /// NOT every position an ordinary name reaches: §14.3.1.1 makes
    /// `let` a Syntax Error in the BoundNames of a `let` or `const`
    /// declaration in sloppy code TOO, so that caller keeps refusing
    /// it and only the `var` spelling asks.
    pub(super) fn let_reads_as_ident(&self) -> bool {
        !self.strict_here()
    }

    /// §13.16 / §14.3.1 — whether a `let` at STATEMENT START heads a
    /// LexicalDeclaration rather than an ExpressionStatement.
    ///
    /// The two readings are only ever in competition where the word
    /// can be a name at all, so strict code answers `true` outright.
    /// In sloppy code the next token decides, and the ASI restriction
    /// §13.16 already wrote the answer down: an ExpressionStatement
    /// may not begin `let [`, which is why the bracket counts as a
    /// declaration here even though `let[0]` is a perfectly ordinary
    /// index expression anywhere else.
    ///
    /// `Token::Let` itself stays on the declaration side so that
    /// `let let = 1` keeps reaching the loud reject §14.3.1.1 wants;
    /// reading it as an expression would have quietly turned it into
    /// two statements.
    pub(super) fn let_begins_declaration(&self) -> bool {
        !self.let_reads_as_ident()
            || matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(
                    Token::Ident(_)
                        | Token::LBracket
                        | Token::LBrace
                        | Token::Yield
                        | Token::Let
                        // §13.1.1 — `let await = 1` is a declaration
                        // under every goal (a Script binds the name, a
                        // Module rejects it at the goal gate). Left out
                        // of this list it read as an ExpressionStatement
                        // `let` followed by a stray `await`, which is
                        // the silent misread, not the reject.
                        | Token::Await
                        // Same for the two words nobody reserves:
                        // `let async = 1` is a LexicalDeclaration, and
                        // without them here it read as `let` the name
                        // followed by a stray `async`. `const type = 1`
                        // never had the problem — only `let` has two
                        // readings to choose between.
                        | Token::Async
                        | Token::Type,
                )
            )
    }

    /// Judge `name` in an IdentifierReference position: `Err` when the
    /// position is already known to be strict, otherwise the site is
    /// recorded for the goal gate and `Ok` admits it.
    ///
    /// The seven-word test runs FIRST because this is the hottest judge
    /// in the parser — every identifier the program mentions arrives
    /// here. An ordinary name pays seven length-then-bytes comparisons
    /// and leaves without touching the site vector, which is noise next
    /// to the `String` clone the same arm makes to build `Expr::Ident`.
    ///
    /// Strictness has a third source at this position that the binding
    /// sites do not see: a class body is strict by §15.7 whatever the
    /// goal and whatever enclosing function said — which is
    /// [`Parser::strict_here`], the one predicate that now spells it.
    pub(super) fn note_strict_reference(&mut self, name: &str) -> Result<(), String> {
        self.refuse_ident_spelled_reserved(name)?;
        if !refused_as_reference(name) {
            return Ok(());
        }
        let strict = self.strict_here();
        self.reject_if_strict_reserved(name, strict)?;
        self.record_strict_goal_site(name);
        Ok(())
    }

    /// Reference-position judge — the §12.7.2 half only. `eval` and
    /// `arguments` stay readable in strict code, so they are absent
    /// here by design.
    pub(super) fn reject_if_strict_reserved(&self, name: &str, strict: bool) -> Result<(), String> {
        if strict && refused_as_reference(name) {
            return Err(format!(
                "`{name}` is a reserved word in strict code at {} (ES §12.7.2)",
                self.at()
            ));
        }
        Ok(())
    }
}
