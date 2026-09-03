//! §16.2 ModuleItemList — the two productions a Statement cannot reach.
//!
//! `ImportDeclaration` and `ExportDeclaration` are ModuleItems, not
//! Statements. The grammar produces them from `ModuleItemList` and
//! from nowhere else, so a statement body — which is a StatementList
//! position — cannot hold one at all:
//!
//! ```text
//! if (false) export default null;              // SyntaxError
//! for (const x = 0; false;) import v from "m"; // SyntaxError
//! { import v from "m"; }                       // SyntaxError
//! ```
//!
//! These are early errors of the parse phase, not of evaluation: the
//! bodies above never run, and test262's `negative: phase: parse`
//! says so by refusing the program before `$DONOTEVALUATE()` matters.
//!
//! **The range this needs is already in a stream.** "Am I at the
//! module's top level" is answered by `stmt_depth`, which
//! `parse_stmt` increments on the way in: a statement `parse_program`
//! asked for sits at depth 1, and everything nested is deeper.
//! Threading a fresh `at_module_top` flag through the twenty-odd
//! nested `parse_stmt` call sites would be twenty-odd chances to
//! forget one.
//!
//! What this gate does NOT answer is the goal question — whether the
//! source is a Module at all. `eval("import v from 'm'")` is a Script
//! and so also a SyntaxError, but by §11.2 rather than by position,
//! and the sub-parser it runs under starts its own `stmt_depth`.
//! Different question, different judge; registered, not folded in.

use super::*;

impl Parser<'_> {
    /// Routes `import` / `export` in statement position, or refuses
    /// them where the position is not a ModuleItem one.
    ///
    /// `None` means the cursor holds neither — including the
    /// `import(...)` / `import.defer(...)` / `import.source(...)`
    /// EXPRESSION forms, which are ImportCalls (§13.3.10 plus the
    /// phase-import proposals) and legal in any statement position.
    /// Those fall through to the expression-statement tail, whose
    /// primary tier owns them.
    pub(super) fn try_parse_module_item(&mut self) -> Option<Result<Stmt, String>> {
        let kw = if matches!(self.peek(), Token::Import)
            && !matches!(self.tokens[self.pos + 1].token, Token::LParen)
            && !(matches!(self.tokens[self.pos + 1].token, Token::Dot)
                && matches!(&self.tokens[self.pos + 2].token,
                    Token::Ident(n) if n == "defer" || n == "source"))
        {
            "import"
        } else if matches!(self.peek(), Token::Export) {
            "export"
        } else {
            return None;
        };
        if self.stmt_depth != 1 {
            return Some(Err(format!(
                "an `{kw}` declaration may only appear at the top level of a module"
            )));
        }
        Some(if kw == "import" {
            self.parse_import()
        } else {
            self.parse_export()
        })
    }
}
