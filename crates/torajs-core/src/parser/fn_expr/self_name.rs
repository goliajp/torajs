//! The optional BindingIdentifier of a function expression, split out
//! of [`super`] at the file-size line.

use super::*;

impl<'a> Parser<'a> {
    /// Optional self-name — chunk 796: recorded by ExprId for the
    /// NamedEvaluation registry (`.name` / fn-print; ES §15.5.5 — the
    /// self-name wins over a binding name). Body-scope self-binding
    /// stays out of scope for the subset.
    pub(super) fn parse_fn_expr_self_name(
        &mut self,
        is_generator: bool,
        is_async: bool,
    ) -> Option<String> {
        if let Token::Ident(n) = self.peek() {
            let n = n.clone();
            self.pos += 1;
            return Some(n);
        }
        if matches!(self.peek(), Token::Yield)
            && !is_generator
            && self.class_stack.is_empty()
            && !self.in_strict_fn
        {
            // The one admission site that cannot share
            // `yield_reads_as_ident`: §15.2 names a FunctionExpression's
            // BindingIdentifier [~Yield], so `(function yield() {})` is
            // legal even INSIDE an enclosing generator, and the
            // predicate's clause is the enclosing bit. A generator
            // EXPRESSION's name is [+Yield] (§15.5), which is why this
            // reads its own `is_generator`. The other two clauses are
            // the predicate's, spelled out and kept in step with it.
            let at = self.at();
            self.ast.yield_ident_positions.push(at);
            self.pos += 1;
            return Some("yield".to_string());
        }
        // §15.2 / §15.8 — the same asymmetry for `await`, on its own
        // axis: a FunctionExpression's BindingIdentifier is [~Await],
        // so `(function await(){})` is legal even inside an async
        // body, while an AsyncFunctionExpression's is [+Await] and is
        // not. That is why this reads the call's `is_async` rather
        // than `await_reads_as_ident`, exactly as the yield clause
        // above reads its own `is_generator`.
        if matches!(self.peek(), Token::Await) && !is_async {
            let at = self.at();
            self.ast.await_ident_positions.push(at);
            self.pos += 1;
            return Some("await".to_string());
        }
        // §12.7.2 — a sloppy function expression may be named `let`,
        // and `async` / `type` are on nobody's reserved list. Written
        // out rather than routed through `pattern_binding_name`
        // because the two arms above answer `yield` and `await` on
        // this position's OWN axes (§15.2's [~Yield] / [~Await]),
        // which the general predicates cannot see.
        if matches!(self.peek(), Token::Let) && self.let_reads_as_ident() {
            self.record_strict_goal_site("let");
            self.pos += 1;
            return Some("let".to_string());
        }
        if let Some(n) = Self::contextual_binding_name(self.peek()) {
            self.pos += 1;
            return Some(n.to_string());
        }
        None
    }
}
