//! When `await` reads as an ordinary name — §15.8's `[Await]`
//! grammar parameter, split out of [`super::strict_reserved`] at
//! rotation 620 (file-size cap). Its sibling answers the same
//! question for `let` and `yield`; `await` needs three times the
//! room because the top-level case is settled by the NEXT token, and
//! that token set is written out here.

use super::*;

impl Parser<'_> {
    /// Whether an `await` in a BINDING position here is an ordinary
    /// name — `yield`'s sibling, judged on the one axis that actually
    /// governs this word.
    ///
    /// `await` is NOT on §12.7.2's strict list: `var await = 1` is a
    /// perfectly ordinary declaration in strict Script code, and the
    /// only grammar that reserves the word is the Module goal
    /// (§13.1.1: `await` is not a valid BindingIdentifier when the goal
    /// symbol is Module) plus the [Await] parameter an async body
    /// carries (§15.8.1). So this reads `await_reserved` and nothing
    /// about strictness, and the goal half is left to
    /// `ast::triage_await_idents` because the goal bit is stamped after
    /// the parse.
    ///
    /// Deliberately NOT `await_allowed`: that one is `true` at a
    /// program's top level so a module may top-level-await, and asking
    /// it here would refuse exactly the position test262's
    /// `await-in-global.js` is about.
    pub(super) fn await_reads_as_ident(&self) -> bool {
        !self.await_reserved
    }

    /// At a program's TOP LEVEL both readings of `await` are
    /// grammatical and the goal bit that picks between them is not
    /// stamped yet: an AwaitExpression under the Module goal, an
    /// IdentifierReference under the Script goal. What separates them
    /// is the token AFTER the word — a name may be followed by an
    /// operator or a closer, an AwaitExpression may not, because its
    /// operand has to begin right there.
    ///
    /// So this lists only tokens that CANNOT begin a UnaryExpression.
    /// Every genuinely ambiguous prefix stays OUT and keeps the
    /// operator reading: `+ - ! ~` and `++ --` (`await -x` is a real
    /// AwaitExpression), `/` (division or a regex), a template's
    /// backtick (`await` could be tagging it). Those spellings are the
    /// ones a Script cannot write anyway — two adjacent expressions —
    /// so nothing legal is lost by reading them as the operator and
    /// letting the module-goal gate speak for the rest.
    fn await_next_token_ends_a_name(&self) -> bool {
        matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            None | Some(
                Token::RParen
                    | Token::RBrace
                    | Token::RBracket
                    | Token::Comma
                    | Token::Semi
                    | Token::Colon
                    | Token::Dot
                    | Token::QuestionDot
                    | Token::Question
                    | Token::QuestionQuestion
                    | Token::Eq
                    | Token::EqEq
                    | Token::EqEqEq
                    | Token::BangEq
                    | Token::BangEqEq
                    | Token::PlusEq
                    | Token::MinusEq
                    | Token::StarEq
                    | Token::SlashEq
                    | Token::PercentEq
                    | Token::StarStarEq
                    | Token::Star
                    | Token::StarStar
                    | Token::Percent
                    | Token::Amp
                    | Token::AmpAmp
                    | Token::Pipe
                    | Token::PipePipe
                    | Token::Caret
                    | Token::ShlShl
                    | Token::ShrShr
                    | Token::ShrShrShr
                    | Token::Lt
                    | Token::Gt
                    | Token::LtEq
                    | Token::GtEq
                    | Token::InstanceOf
                    | Token::FatArrow
                    | Token::Eof
            )
        )
    }

    /// Whether an `await` at THIS expression position reads as a name.
    /// Off wherever the grammar's `[Await]` parameter is on; otherwise
    /// true when no AwaitExpression could stand here at all, and at a
    /// program's top level when the next token settles it.
    pub(super) fn await_reads_as_ref(&self) -> bool {
        self.await_reads_as_ident() && (!self.await_allowed || self.await_next_token_ends_a_name())
    }

    /// Record an admitted binding-position `await` for the module-goal
    /// gate and return the name the caller binds.
    pub(super) fn admit_await_ident(&mut self) -> String {
        let at = self.at();
        self.ast.await_ident_positions.push(at);
        "await".to_string()
    }
}
