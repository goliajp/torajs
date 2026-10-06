//! §B.3.1 — an object literal may set [[Prototype]] through at most
//! one `__proto__: v` (the `PropertyName : AssignmentExpression`
//! production with a non-computed `__proto__` name). Shorthand,
//! method, accessor and computed `__proto__` keys define ordinary own
//! properties and do not count. The rule binds only a literal that
//! stays an expression: re-read as an assignment pattern, duplicate
//! `__proto__` targets are legal, so the parser records the extra
//! entries and the checker refuses them where the literal is typed.

use super::*;
use crate::ast::PropKey;

impl<'a> Parser<'a> {
    /// Sort the field that started at `start` and was keyed `name`:
    /// spelled `__proto__ :` (identifier, escaped identifier or string
    /// key) it is a [[Prototype]] set and joins `protos`; any other
    /// `__proto__` member (method, generator, async method) is an own
    /// property, which the shorthand side table already tells the
    /// dynobj-init lane.
    pub(super) fn note_proto_colon(
        &mut self,
        start: usize,
        (name, value): (&PropKey, ExprId),
        protos: &mut Vec<ExprId>,
    ) {
        if *name != "__proto__" {
            return;
        }
        let named = match self.tokens.get(start).map(|t| &t.token) {
            Some(Token::Ident(s)) => s == "__proto__",
            Some(Token::String(s)) => *s == "__proto__",
            _ => false,
        };
        let colon = matches!(
            self.tokens.get(start + 1).map(|t| &t.token),
            Some(Token::Colon)
        );
        if named && colon {
            protos.push(value);
        } else {
            self.ast.objlit_shorthand_proto_exprs.insert(value);
        }
    }
}
