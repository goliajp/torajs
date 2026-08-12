//! The top-level program loop — the one statement stream that is not
//! a function body, and therefore the one whose directive prologue
//! answers §11.2.2's question about GLOBAL code rather than function
//! code.

use super::*;

impl Parser<'_> {
    pub(super) fn parse_program(&mut self) -> Result<(), String> {
        // §11.2.2 — global code is strict mode code when it opens with
        // a Use Strict Directive, and everything nested inherits that.
        // Tracked here rather than read back off `ast.stmts` for the
        // reason `arm_strict_directive_program` gives.
        let mut in_prologue = true;
        while !matches!(self.peek(), Token::Eof) {
            let stmt = self.parse_stmt()?;
            self.arm_strict_directive_program(&stmt, &mut in_prologue);
            // P8.5 — flush parser-synthesized class expressions
            // (`__ClassExpr_<id>` ClassDecls produced by class-in-
            // expression-position) immediately before the stmt that
            // owns their use site. Preserves: (i) parent-before-child
            // for `class extends Parent` (Parent was pushed in an
            // earlier iteration); (ii) synth-before-use (Ident hop in
            // the about-to-be-pushed stmt). Append-only — does not
            // alter the `stmt_offset` contract that `parse_into` relies
            // on for module merging.
            if !self.synth_classes.is_empty() {
                let synth: Vec<Stmt> = std::mem::take(&mut self.synth_classes);
                self.ast.stmts.extend(synth);
            }
            self.ast.stmts.push(stmt);
        }
        Ok(())
    }
}
