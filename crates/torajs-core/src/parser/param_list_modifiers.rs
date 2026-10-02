//! TS parameter-property modifiers on a constructor parameter.
//!
//! Moved out of `param_list` under the 500-line file rule: that file
//! was at 496 when the §13.1.1 `await` arm needed four lines in
//! `parse_ctor_param_list`, and a file with four lines of headroom has
//! no room to grow a helper either — the only cut left is to move a
//! family out whole.
//!
//! The line is one the parser already draws: `param_list` answers how
//! a parameter LIST is walked, and this answers what modifiers a
//! single constructor parameter carries in front of its name.

use super::*;

impl Parser<'_> {
    /// Consume any TS modifiers on a ctor parameter: visibility
    /// (`public` / `private` / `protected`) and `readonly`. Order is
    /// visibility-then-readonly per TS, but any combination is
    /// accepted once. Returns the pair the caller records on the
    /// promoted-field side-table.
    pub(super) fn consume_ctor_param_modifiers(
        &mut self,
    ) -> Result<(Option<ast::Visibility>, bool), String> {
        let mut vis: Option<ast::Visibility> = None;
        let mut rd = false;
        while let Token::Ident(s) = self.peek() {
            match s.as_str() {
                "public" => {
                    if vis.is_some() {
                        return Err(format!(
                            "duplicate visibility modifier in ctor param at {}",
                            self.at()
                        ));
                    }
                    vis = Some(ast::Visibility::Public);
                    self.pos += 1;
                }
                "private" => {
                    if vis.is_some() {
                        return Err(format!(
                            "duplicate visibility modifier in ctor param at {}",
                            self.at()
                        ));
                    }
                    vis = Some(ast::Visibility::Private);
                    self.pos += 1;
                }
                "protected" => {
                    if vis.is_some() {
                        return Err(format!(
                            "duplicate visibility modifier in ctor param at {}",
                            self.at()
                        ));
                    }
                    vis = Some(ast::Visibility::Protected);
                    self.pos += 1;
                }
                "readonly" => {
                    if rd {
                        return Err(format!(
                            "duplicate `readonly` in ctor param at {}",
                            self.at()
                        ));
                    }
                    rd = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        Ok((vis, rd))
    }
}
