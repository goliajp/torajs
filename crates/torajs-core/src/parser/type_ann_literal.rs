//! Literal type annotations — `"dev" | "prod"`, `0 | 1`, `true`
//! (TS §3.2.10: the type whose only value is that literal).
//!
//! Every annotation the rest of the program reads spells a literal
//! type as its base type: `string`, `number`, `boolean`, a `|`-chain
//! of further literals of the same kind swallowed into the same base.
//! A `type` / `interface` declaration is read twice over: with each
//! literal as its marker (`check_type_ann::literal_marker`), recorded
//! for the checker in `Ast::literal_type_decls`, and widened from that
//! into the declaration's AST fields.

use super::*;
use crate::ast::PropKey;
use crate::check::Lit;

impl Parser<'_> {
    /// The annotation of the literal type at the cursor, or `None`
    /// when the next token does not start one.
    pub(super) fn literal_type_marker(&mut self) -> Option<String> {
        let lit = match self.peek() {
            Token::String(s) => Lit::Str(s.clone()),
            Token::Number(n) => Lit::Num(*n),
            Token::True => Lit::Bool(true),
            Token::False => Lit::Bool(false),
            _ => return None,
        };
        self.pos += 1;
        if self.keep_literals {
            return Some(crate::check_type_ann::literal_marker(&lit));
        }
        // a chain of further literals of the same kind widens to the
        // same base
        while matches!(self.peek(), Token::Pipe) {
            let next = self.tokens.get(self.pos + 1).map(|s| &s.token);
            let same = match (&lit, next) {
                (Lit::Str(_), Some(Token::String(_))) => true,
                (Lit::Num(_), Some(Token::Number(_))) => true,
                (Lit::Bool(_), Some(Token::True | Token::False)) => true,
                _ => false,
            };
            if !same {
                break;
            }
            self.pos += 2;
        }
        Some(
            match lit {
                Lit::Str(_) => "string",
                Lit::Num(_) => "number",
                Lit::Bool(_) => "boolean",
            }
            .to_string(),
        )
    }

    /// Parses one `type` / `interface` declaration with `parse`, its
    /// literal types kept, and records them; the declaration's fields
    /// come back widened.
    pub(super) fn with_literal_types(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<Stmt, String>,
    ) -> Result<Stmt, String> {
        let was = std::mem::replace(&mut self.keep_literals, true);
        let parsed = parse(self);
        self.keep_literals = was;
        let mut stmt = parsed?;
        if let Stmt::TypeDecl { name, fields, .. } = &mut stmt {
            let widened: Vec<(PropKey, String)> = fields
                .iter()
                .map(|(k, t)| (k.clone(), crate::check_type_ann::widen_literal_types(t)))
                .collect();
            let precise = std::mem::replace(fields, widened);
            let wrote_literal = precise.iter().zip(fields.iter()).any(|(p, w)| p.1 != w.1);
            let record = &mut self.ast.literal_type_decls;
            // a second declaration of the name (a nested scope, a
            // merged module) makes the record ambiguous: none is kept
            match record.get_mut(name) {
                Some(slot) => *slot = None,
                None => {
                    record.insert(name.clone(), wrote_literal.then_some(precise));
                }
            }
        }
        Ok(stmt)
    }
}
