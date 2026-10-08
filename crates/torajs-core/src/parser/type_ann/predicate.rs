//! Return-type predicates: the type predicate `v is T` (TS §3.6.5)
//! and the assertion signature `asserts v` / `asserts v is T` (TS 3.7).
//!
//! Both describe a narrow the caller makes, not a value: a predicate
//! returns `boolean`, an assertion returns nothing.

use super::*;

impl Parser<'_> {
    /// The predicate at the cursor, consumed, as the return type it
    /// stands for; `None` (nothing consumed) when there is none.
    pub(super) fn parse_predicate_sig(&mut self) -> Option<Result<String, String>> {
        let next = self.tokens.get(self.pos + 1).map(|s| &s.token);
        if matches!(self.peek(), Token::Ident(s) if s == "asserts")
            && matches!(next, Some(Token::Ident(_) | Token::This))
        {
            self.pos += 2;
            if matches!(self.peek(), Token::Ident(s) if s == "is") {
                self.pos += 1;
                if let Err(e) = self.parse_type_ann() {
                    return Some(Err(e));
                }
            }
            return Some(Ok("void".to_string()));
        }
        if matches!(self.peek(), Token::Ident(_) | Token::This)
            && matches!(next, Some(Token::Ident(s)) if s == "is")
        {
            self.pos += 2;
            return Some(self.parse_type_ann().map(|_| "boolean".to_string()));
        }
        None
    }
}
