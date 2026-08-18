//! Tagged template lowering — `tag`s0${e1}s1`` (§13.3.11).
//!
//! ES semantics: the call receives the SITE's template object first
//! and the substitution values after it, uncoerced —
//! `tag(templateObject, e1, …)`. The template object is built by
//! GetTemplateObject (§13.2.8.4): a frozen array of the cooked
//! strings whose `.raw` property is a frozen array of the raw
//! strings, cached PER SITE so every evaluation of the same
//! TemplateLiteral answers the identical object.
//!
//! Desugar shape: the template object spells as a synthetic call
//! every downstream pass already knows how to walk —
//!
//! ```text
//! __torajs_template_object(<site>, c0, r0, c1, r1, …)
//! ```
//!
//! where `<site>` is a placeholder `-1` here; the
//! `ast::number_template_sites` pass renumbers every site in arena
//! order after modules splice (parse-order deterministic across the
//! whole program — a per-Parser counter would collide across
//! modules). The checker answers the synthetic callee `Any`; the
//! lowering bakes the string literals static and hands them to the
//! runtime template-object kernel. The tag call itself is an
//! ordinary `Expr::Call`, so a member tag (`obj.tag`…`) keeps the
//! method-call receiver semantics for free.

use super::*;

/// The synthetic callee name every layer keys on.
pub(crate) const TEMPLATE_OBJECT_CALLEE: &str = "__torajs_template_object";

impl<'a> Parser<'a> {
    /// Lower `tag` followed by a `Token::Template` at postfix
    /// position into `tag(__torajs_template_object(…), subs…)`.
    pub(super) fn lower_tagged_template(
        &mut self,
        tag: ExprId,
        parts: &[lexer::TemplatePart],
    ) -> Result<ExprId, String> {
        // Collect the alternation into n+1 (cooked, raw) pairs and n
        // substitution expressions. The lexer flushes a literal
        // segment before every interpolation, but elides an empty
        // TAIL segment — normalize both ends here so the pair count
        // is always subs + 1 (what the spec's TemplateStrings gives).
        let mut lits: Vec<(String, String)> = Vec::new();
        let mut subs: Vec<ExprId> = Vec::new();
        for p in parts {
            match p {
                lexer::TemplatePart::Lit { cooked, raw } => {
                    lits.push((cooked.clone(), raw.clone()));
                }
                lexer::TemplatePart::Expr(tokens) => {
                    if lits.len() == subs.len() {
                        // Leading interpolation with no flushed
                        // empty head (defensive — the lexer does
                        // flush it today).
                        lits.push((String::new(), String::new()));
                    }
                    subs.push(self.parse_template_interpolation(tokens)?);
                }
            }
        }
        if lits.len() == subs.len() {
            lits.push((String::new(), String::new()));
        }
        // `__torajs_template_object(-1, c0, r0, …)` — site renumbered
        // by `ast::number_template_sites`.
        let mut tmpl_args: Vec<ExprId> = Vec::with_capacity(1 + lits.len() * 2);
        tmpl_args.push(self.ast.add_expr(Expr::Number(-1.0)));
        for (c, r) in lits {
            tmpl_args.push(self.ast.add_expr(Expr::String(c)));
            tmpl_args.push(self.ast.add_expr(Expr::String(r)));
        }
        let tmpl_callee = self
            .ast
            .add_expr(Expr::Ident(TEMPLATE_OBJECT_CALLEE.to_string()));
        // §13.2.8.4 builds the object from the parse node, not a
        // name lookup — no `with` object may supply this callee.
        self.ast.synth_global_refs.insert(tmpl_callee);
        let tmpl = self.ast.add_expr(Expr::Call {
            callee: tmpl_callee,
            args: tmpl_args,
        });
        let mut args = vec![tmpl];
        args.extend(subs);
        Ok(self.ast.add_expr(Expr::Call { callee: tag, args }))
    }
}
