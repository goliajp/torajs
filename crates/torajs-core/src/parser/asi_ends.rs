//! §12.10.1 — where a production that ends in `;` may end without one.
//!
//! ASI inserts the semicolon before an offending token only when a
//! LineTerminator precedes it, when it is `}`, or at end of input. A
//! production that merely skips an optional `;` reads `var s = '' ''`
//! as a declaration followed by a stray expression statement, and
//! `field = 1 method() {}` as a field followed by a method — two
//! programs nobody wrote.

use super::*;

impl Parser<'_> {
    /// Consume the `;` that ends `what`, or confirm ASI may insert it.
    pub(super) fn expect_asi_end(&mut self, what: &str) -> Result<(), String> {
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            return Ok(());
        }
        if matches!(self.peek(), Token::Eof | Token::RBrace) || self.has_newline_before(self.pos) {
            return Ok(());
        }
        Err(format!(
            "expected `;` or a line break after {what}, got {:?} at {} (ES §12.10.1)",
            self.peek(),
            self.at()
        ))
    }
}
