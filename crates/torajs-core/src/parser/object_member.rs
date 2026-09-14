//! Object-literal async method shorthand — `{ async name() { … } }`.
//!
//! §13.2.5 makes a method shorthand sugar for a function-valued
//! property, and an async one is an async function VALUE: the
//! property's value is the same `Expr::ArrowFn` the computed-key
//! spelling (`async [k]() {}`) and the sync named spelling mint, put
//! in `async_fn_value_exprs` so the async desugar's value lane moves
//! it — a closure over the enclosing scope, `this` through the
//! receiver channel.
//!
//! History: P10.3-A3b replaced a body-dropping stub with a hoisted
//! top-level `__obj_async_method_<id>` declaration referenced by
//! name (the async desugar then only reached statement-level
//! FnDecls). That shape captured nothing — `{ async next() { i++ } }`
//! inside a method read `i` as an unknown identifier — and rotation
//! 646 retired it once the value lane existed.

use super::Parser;
use crate::ast::ExprId;
use crate::ast::PropKey;
use crate::lexer::Token;

impl<'a> Parser<'a> {
    /// Try to parse an `async <Ident>(...) {...}` method shorthand.
    /// Returns `Ok(Some((field_name, value)))` when the lookahead
    /// matched and the method was fully consumed, `Ok(None)` when
    /// the leading token wasn't an async-method shape (caller falls
    /// through to existing paths — incl. computed-key stub-drop and
    /// regular `async` as a field-name ident).
    pub(super) fn try_parse_async_object_method_shorthand(
        &mut self,
    ) -> Result<Option<(PropKey, ExprId)>, String> {
        // Match `async <Ident> (`. Computed-key `async [...]()` stays
        // on the stub-drop path in the caller; explicitly NOT matched
        // here so `Ok(None)` falls through.
        if !matches!(self.peek(), Token::Async) {
            return Ok(None);
        }
        // Span anchor -- the `async` token (B1b); nothing is consumed
        // before the lookahead guards return None, so capturing here
        // is safe.
        let start_pos = self.pos;
        let Some(t1) = self.tokens.get(self.pos + 1) else {
            return Ok(None);
        };
        // §12.7.6 PropertyName is an IdentifierName, so an async
        // shorthand may be named with a keyword (`{ async if() {} }`,
        // `{ async let() {} }`). Matching `Token::Ident` alone sent
        // every one of them back to the field path, which then
        // demanded a `:` after the word `async`. The generator
        // spelling `{ async *if() {} }` already worked.
        let method_name = match &t1.token {
            Token::Ident(name) => name.clone(),
            t => match Self::keyword_property_name(t) {
                Some(kw) => kw.to_string(),
                None => return Ok(None),
            },
        };
        let Some(t2) = self.tokens.get(self.pos + 2) else {
            return Ok(None);
        };
        if !matches!(t2.token, Token::LParen) {
            return Ok(None);
        }
        // Consume `async` and the method name.
        self.pos += 2;

        // Params — reuse the standard param parser (handles
        // destructuring + type anns).
        // This form IS the async shorthand, so its params are [+Await].
        // The method is an async function VALUE, the shape the
        // computed-key spelling already takes (`[k]() {…}` with
        // `async`): a closure over the enclosing scope, moved by the
        // async desugar's value lane. It used to be minted as a
        // hoisted top-level declaration named `__obj_async_method_N`
        // and referenced by name — which captured nothing, so
        // `{ async next() { i++ } }` inside a method read an unknown
        // identifier (rotation 646).
        let value = self.parse_method_like_value_async(
            start_pos,
            true,
            &format!("async object-method `{method_name}`"),
            true,
        )?;
        self.ast.async_fn_value_exprs.insert(value);
        Ok(Some((PropKey::from(method_name), value)))
    }
}
