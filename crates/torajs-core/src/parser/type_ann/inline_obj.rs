//! Object type literals — `{ x: T; y: U }` and the index signature
//! `{ [k: string]: V }` (TS §3.9.4).
//!
//! A literal without an index signature is the struct shape
//! `__inlobj(x:T|y:U)`, decoded into a `Type::Struct` downstream (same
//! encoding scheme as `__fn(...)`). One with an index signature is a
//! dictionary, spelled `__dict(K|V)`: the key type and the value type.
//! The named members declared beside the signature only have to fit
//! it; they go to `Ast::dict_sites` with the key, where the checker
//! diagnoses both. The `Record<K, V>` utility type is the same
//! dictionary ([`Parser::record_dict_ann`]).

use super::*;

mod dict;

/// One named member of an object type body: name, annotation, the
/// annotation's source text and the name's byte offset.
type ObjTypeMember = (String, String, String, u32);

/// An index signature `[k: K]: V`: key annotation, its source text and
/// the parameter name's byte offset; value annotation and source text.
struct IndexSig {
    key: String,
    key_src: String,
    key_at: u32,
    value: String,
    value_src: String,
}

/// An object type body: its named members and its index signature.
type ObjTypeMembers = (Vec<ObjTypeMember>, Option<IndexSig>);

impl Parser<'_> {
    /// The object type literal at the cursor (`{` not yet consumed).
    pub(super) fn parse_inline_obj_type(&mut self) -> Result<String, String> {
        self.pos += 1;
        let (fields, index_sig) = self.parse_obj_type_members(Token::RBrace, "inline obj type")?;
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end inline obj type, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut name = match index_sig {
            Some(sig) => self.dict_ann(sig, fields, false),
            None => {
                let fields: Vec<String> =
                    fields.iter().map(|(n, t, ..)| format!("{n}:{t}")).collect();
                format!("__inlobj({})", fields.join("|"))
            }
        };
        // V3-18 wedge — `{ ... } | null` shape. Mirror the
        // post-Ident pipe handler for the inline-obj case.
        // An array suffix does not continue past a line break.
        while matches!(self.peek(), Token::LBracket)
            && !self.has_newline_before(self.pos)
            && matches!(
                self.tokens.get(self.pos + 1).map(|s| &s.token),
                Some(Token::RBracket)
            )
        {
            self.pos += 2;
            name.push_str("[]");
        }
        self.consume_union_suffix(name)
    }

    /// The members of an object type body up to (not consuming)
    /// `close`: the named members, and the index signature when there
    /// is one. With a `string` and a `number`
    /// signature both present the `string` one is the dictionary's
    /// (TS requires the `number` one's value to fit it).
    fn parse_obj_type_members(
        &mut self,
        close: Token,
        what: &str,
    ) -> Result<ObjTypeMembers, String> {
        let mut fields: Vec<ObjTypeMember> = Vec::new();
        let mut index_sig: Option<IndexSig> = None;
        while *self.peek() != close {
            // `readonly` on a member is type-side only: accepted and
            // discarded.
            if let Token::Ident(s) = self.peek()
                && s == "readonly"
                && let Some(next) = self.tokens.get(self.pos + 1)
                && matches!(next.token, Token::Ident(_) | Token::LBracket)
            {
                self.pos += 1;
            }
            if let Some(sig) = self.parse_index_sig()? {
                if index_sig.as_ref().is_none_or(|prev| prev.key != "string") {
                    index_sig = Some(sig);
                }
            } else {
                fields.push(self.parse_obj_type_field(what)?);
            }
            match self.peek() {
                Token::Comma | Token::Semi => self.pos += 1,
                t if *t == close => break,
                _ if self.has_newline_before(self.pos) => {}
                t => {
                    return Err(format!(
                        "expected `,` `;` or `}}` in {what}, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        Ok((fields, index_sig))
    }

    /// `[k: K]: V` at the cursor, consumed; `None` (nothing consumed)
    /// when the cursor is not at one.
    fn parse_index_sig(&mut self) -> Result<Option<IndexSig>, String> {
        let at = |p: &Self, i: usize| p.tokens.get(p.pos + i).map(|t| &t.token);
        if !(matches!(self.peek(), Token::LBracket)
            && matches!(at(self, 1), Some(Token::Ident(_)))
            && matches!(at(self, 2), Some(Token::Colon)))
        {
            return Ok(None);
        }
        let key_at = self.tokens[self.pos + 1].span.start;
        self.pos += 3;
        let (key, key_src) = self.parse_type_ann_src()?;
        for (tok, what) in [(Token::RBracket, "`]`"), (Token::Colon, "`:`")] {
            if *self.peek() != tok {
                return Err(format!(
                    "expected {what} in index signature, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
            self.pos += 1;
        }
        let (value, value_src) = self.parse_type_ann_src()?;
        Ok(Some(IndexSig {
            key,
            key_src,
            key_at,
            value,
            value_src,
        }))
    }

    /// An annotation with its source text.
    fn parse_type_ann_src(&mut self) -> Result<(String, String), String> {
        let start = self.pos;
        let ann = self.parse_type_ann()?;
        let span = self.span_from(start);
        Ok((
            ann,
            self.source[span.start as usize..span.end as usize].to_string(),
        ))
    }

    /// One named member `name: T` / `name?: T` / `m(p: T): R`.
    fn parse_obj_type_field(&mut self, what: &str) -> Result<ObjTypeMember, String> {
        let at = self.at();
        let fname = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected field name in {what}, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // `field?: T` (TS §3.9.2): the value is `T | undefined`, and the
        // name keeps the `?` (`__inlobj(field?:T)`), since only an
        // optional member may be absent while a `T | null` one may not
        let optional = matches!(self.peek(), Token::Question);
        if optional {
            self.pos += 1;
        }
        // `m(p: T): R` — a MethodSignature, which TS reads as the
        // property holding a `(p: T) => R`, so nothing downstream
        // learns a new shape.
        let (fty_raw, src) = if matches!(self.peek(), Token::LParen) {
            let start = self.pos;
            let ann = self.parse_method_sig_type_ann()?;
            let span = self.span_from(start);
            (
                ann,
                self.source[span.start as usize..span.end as usize].to_string(),
            )
        } else {
            match self.peek() {
                Token::Colon => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `:` after inline obj field name, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            self.parse_type_ann_src()?
        };
        // Chunk 793 — struct-field fn slots are Closure-repr. Retag at
        // birth: this is the site that mints `__inlobj(` from syntax,
        // and nested inline objects recurse through here, so every
        // downstream consumer sees the same `__cls(` field the
        // named-TypeDecl lane sees.
        let fty_raw = crate::ast::retag_field_fn_ann(&fty_raw);
        if !optional {
            return Ok((fname, fty_raw, src, at));
        }
        // `a??`: the written type spells `null` / `undefined` itself, so
        // the member may hold `null`; `a?` is the plain optional member,
        // whose `number` keeps an f64 slot (`ssa_lower_parse_type`).
        if fty_raw.starts_with("__nullable(") {
            return Ok((format!("{fname}??"), fty_raw, src, at));
        }
        Ok((
            format!("{fname}?"),
            format!("__nullable({fty_raw})"),
            src,
            at,
        ))
    }

    /// Whether the object type body opening at the `{` under the
    /// cursor declares an index signature among its own members.
    pub(in crate::parser) fn brace_body_has_index_sig(&self) -> bool {
        let tok = |i: usize| self.tokens.get(i).map(|t| &t.token);
        let mut depth = 0usize;
        let mut prev: Option<&Token> = None;
        for i in self.pos.. {
            let Some(t) = tok(i) else { return false };
            match t {
                Token::LBracket
                    if depth == 1
                        && (matches!(prev, Some(Token::LBrace | Token::Semi | Token::Comma))
                            || matches!(prev, Some(Token::Ident(s)) if s == "readonly")
                            || self.has_newline_before(i))
                        && matches!(tok(i + 1), Some(Token::Ident(_)))
                        && matches!(tok(i + 2), Some(Token::Colon)) =>
                {
                    return true;
                }
                Token::LBrace | Token::LParen | Token::LBracket => depth += 1,
                Token::RBrace | Token::RParen | Token::RBracket => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                _ => {}
            }
            prev = Some(t);
        }
        false
    }
}
