//! ReservedWords that reach the parser looking like names.
//!
//! §12.7.2 reserves `debugger`, `enum`, `in` and `with` under every goal
//! and every strictness, yet none of the four has a token of its own:
//! `in` is the relational operator the expression parser recognises by
//! spelling, `with` and `debugger` head statements the dispatcher
//! recognises by spelling, and `enum` heads TypeScript's declaration. So
//! the lexer hands them over as `Token::Ident`, and every name-shaped
//! judge has to refuse them itself: §13.1.1 makes it a Syntax Error for
//! an IdentifierReference, BindingIdentifier or LabelIdentifier to have a
//! ReservedWord as its StringValue.
//!
//! The object-literal shorthand is the other way a reserved word turns
//! into a name: its key is read by the property-name reader, which
//! admits the whole reserved list (`{ this: 1 }` is legal), and only the
//! shorthand shape then re-reads the key as an IdentifierReference.
//!
//! The contextual keywords go the opposite way: `of`, `as`, `from`,
//! `static`, `target` are names everywhere except the one production
//! that spells them, and there they are terminal symbols that must
//! appear exactly as written (§5.1.5) — `for (x o\u0066 y)` is no
//! for-of. `at_word` is the test those productions ask.

use super::*;

const IDENT_SPELLED_RESERVED: [&str; 4] = ["debugger", "enum", "in", "with"];

pub(super) fn is_ident_spelled_reserved(name: &str) -> bool {
    IDENT_SPELLED_RESERVED.contains(&name)
}

impl Parser<'_> {
    /// The current token is the word `word` spelled without an escape:
    /// an escape always lengthens the source span past the decoded name.
    pub(super) fn at_word(&self, word: &str) -> bool {
        self.tokens.get(self.pos).is_some_and(|t| {
            matches!(&t.token, Token::Ident(n) if n == word)
                && (t.span.end - t.span.start) as usize == word.len()
        })
    }

    pub(super) fn refuse_ident_spelled_reserved(&self, name: &str) -> Result<(), String> {
        if is_ident_spelled_reserved(name) {
            return Err(format!(
                "`{name}` is a reserved word and cannot be used as an identifier at {} \
                 (ES §13.1.1)",
                self.at()
            ));
        }
        Ok(())
    }

    /// §14.16 DebuggerStatement — `debugger ;`. With no debugging
    /// facility attached it evaluates to an empty completion. Only the
    /// literal spelling heads the statement: an escaped one lexes as
    /// `Token::EscapedIdent` and is refused like any escaped keyword.
    pub(super) fn try_parse_debugger(&mut self) -> Option<Result<Stmt, String>> {
        if !matches!(self.peek(), Token::Ident(n) if n == "debugger") {
            return None;
        }
        self.pos += 1;
        Some(self.expect_stmt_end().map(|()| Stmt::Block(Vec::new())))
    }

    /// §13.2.5 PropertyDefinition : IdentifierReference — judge the key
    /// token at `key_pos` of a shorthand property. A plain name, a word
    /// nobody reserves, and `let` (judged by name downstream) pass;
    /// `yield` and `await` pass only where their own predicates read
    /// them as names, recording the site for the goal gate the same way
    /// every other admission does; every other reserved word is refused.
    /// The name itself then goes through the reference judge.
    pub(super) fn judge_shorthand_key(&mut self, key_pos: usize, name: &str) -> Result<(), String> {
        let tok = self.tokens[key_pos].token.clone();
        let admitted = match &tok {
            Token::Ident(_) | Token::Async | Token::Type | Token::Let => true,
            t => match Self::reserved_word_of(t) {
                Some("yield") if self.yield_reads_as_ident() => {
                    let at = self.at();
                    self.ast.yield_ident_positions.push(at);
                    true
                }
                Some("await") if self.await_reads_as_ident() => {
                    self.admit_await_ident();
                    true
                }
                Some("let") => true,
                _ => false,
            },
        };
        if admitted {
            return self.note_strict_reference(name);
        }
        Err(format!(
            "a reserved word cannot be a shorthand property, got {tok:?} at {} (ES §13.1.1)",
            self.at()
        ))
    }
}
