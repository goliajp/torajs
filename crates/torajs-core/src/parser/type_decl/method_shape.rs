//! The method shape of a type-body member (`m(p: T): R`), split out
//! of the type-declaration cluster.

use super::super::*;

impl<'a> Parser<'a> {
    /// V3-18 wedge — method-shape field (`m(p: T): R`) in
    /// an interface / type-decl body. Per TS spec §3.7 the
    /// shape is equivalent to `m: (p: T) => R`. Subset rewrites
    /// by parsing the param list + `: R` and synthesizing the
    /// arrow-fn type-ann string.
    ///
    /// Entered at the `(`; answers the arrow-fn type-ann string.
    pub(super) fn parse_method_shape_ann(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut param_anns: Vec<String> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // Optional `name:` prefix on each param — discarded.
                if matches!(self.peek(), Token::Ident(_))
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|s| &s.token),
                        Some(Token::Colon) | Some(Token::Question)
                    )
                {
                    self.pos += 1;
                    if matches!(self.peek(), Token::Question) {
                        self.pos += 1;
                    }
                    if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                    }
                }
                param_anns.push(self.parse_type_ann()?);
                match self.peek() {
                    Token::Comma => self.pos += 1,
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in method-shape params, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        let ret_ann = match self.peek() {
            Token::Colon => {
                self.pos += 1;
                self.parse_type_ann()?
            }
            _ => "void".to_string(),
        };
        Ok(crate::type_ann_fnsig::fn_type_ann(
            "__fn",
            &param_anns.join("|"),
            &ret_ann,
        ))
    }
}
