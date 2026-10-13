//! A destructuring pattern in a parameter list.

use super::*;

impl<'a> Parser<'a> {
    /// Parse the pattern into `let` bindings off a synthesized param
    /// and return that param's name.
    ///
    /// The bound names are judged as BindingIdentifiers here, where they
    /// are read: §13.1.1 refuses `eval` / `arguments` and §12.7.2 the
    /// strict reserved words (escaped spellings included) in strict
    /// code. A function with a non-simple parameter list cannot carry
    /// its own "use strict" (§15.2.1), so the strictness these names
    /// answer to is already known at this point.
    pub(super) fn parse_destr_param(&mut self, lets: &mut Vec<Stmt>) -> Result<String, String> {
        let synth = format!("__param_destr_{}", self.mint_desugar_id());
        let from = lets.len();
        self.parse_destr_into(synth.clone(), lets)?;
        let strict = self.strict_here();
        for s in &lets[from..] {
            if let Stmt::LetDecl { init, .. } = s {
                self.ast.param_destr_inits.insert(*init);
            }
            if let Stmt::LetDecl { name, .. } = s
                && !name.starts_with("__")
            {
                self.note_strict_binding(name, strict)?;
            }
        }
        Ok(synth)
    }

    /// §13.1.1 / §12.7.2 for the parameter of `x => body`, judged once
    /// the body is read: a block body's own "use strict" makes the
    /// parameter strict too (§15.3.1).
    pub(super) fn judge_bare_arrow_param(
        &mut self,
        name: &str,
        body: &[Stmt],
    ) -> Result<(), String> {
        let own = self.prologue_says_use_strict(body);
        let strict = self.strict_here() || own;
        self.note_strict_binding(name, strict)
    }
}
