//! Type-declaration cluster (chunk 415).
//!
//! Extracted verbatim from parser.rs — the three methods behind
//! TS type-side declarations:
//! - parse_interface_decl — `interface X { ... }`, treated as alias
//!   for `type X = { ... }` (no declaration merging / heritage)
//! - parse_type_decl — `type Name = { f1: T1; f2: T2 };` incl.
//!   type-params, `,` / `;` / ASI field separators
//! - parse_type_decl_field — one `name: T` field with `readonly` /
//!   optional-`?` wedges
//!
//! parse_interface_decl + parse_type_decl are called from
//! parse_stmt (parse_stmt.rs sibling); parse_type_decl_field is
//! internal to this cluster. All promoted `pub(super)` per the
//! sibling-impl pack pattern. Body unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// V3-18 wedge — `interface X { ... }` parsing. Per TS spec
    /// §3.7, interfaces are nominal type-side declarations; the
    /// subset treats them as alias for `type X = { ... }` (no
    /// declaration-merging / heritage clauses are honored beyond
    /// what's already covered by `type`).
    pub(super) fn parse_interface_decl(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `interface`
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected interface name after `interface`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // Optional generic type-parameter list — mirror parse_type_decl.
        let mut type_params: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    match self.peek() {
                        Token::Ident(n) => {
                            type_params.push(n.clone());
                            self.pos += 1;
                        }
                        t => {
                            return Err(format!(
                                "expected type-parameter name in interface<...>, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in interface type params, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::Gt => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `>` to close interface type params, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        // Optional `extends Foo, Bar` clause — subset stub: tokens
        // are consumed and discarded (no field-inheritance yet).
        if matches!(self.peek(), Token::Extends) {
            self.pos += 1;
            loop {
                let _parent = self.parse_type_ann()?;
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin interface body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            fields.push(self.parse_type_decl_field()?);
            while matches!(self.peek(), Token::Comma | Token::Semi) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                fields.push(self.parse_type_decl_field()?);
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end interface body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::TypeDecl {
            name,
            type_params,
            fields,
        })
    }

    /// `type Name = { f1: T1, f2: T2 };`
    pub(super) fn parse_type_decl(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `type`
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected type name after `type`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // M3.4 — optional type parameters: `type Pair<A, B> = { ... }`.
        let mut type_params: Vec<String> = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    match self.peek() {
                        Token::Ident(n) => {
                            type_params.push(n.clone());
                            self.pos += 1;
                        }
                        t => {
                            return Err(format!(
                                "expected type-parameter name in type<...>, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in type params, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::Gt => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `>` to close type params, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::Eq => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `=` after type name, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // V3-18 wedge — bare type alias: `type ID = <type>` (RHS
        // is a non-struct type-ann like `number` / `string[]` /
        // `T | null` / `() => T`). Encoded as
        // Stmt::TypeDecl { fields = [("__alias__", "<ann>")] }
        // so check.rs can detect via the sentinel field name and
        // resolve to the alias's actual Type without wrapping in
        // a Struct. Real struct-shape `{ ... }` keeps the
        // existing Vec<(name, ty)> path untouched.
        if !matches!(self.peek(), Token::LBrace) {
            let ann = self.parse_type_ann()?;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::TypeDecl {
                name,
                type_params,
                fields: vec![("__alias__".to_string(), ann)],
            });
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin type body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            fields.push(self.parse_type_decl_field()?);
            // V3-18 m1.h.54 — TS spec also allows `;` (or newline-implied
            // ASI) as a field separator inside type literals. Pre-fix
            // tora only accepted `,`, hard-rejecting the canonical
            // `type T = { a: number; b: number }` form.
            while matches!(self.peek(), Token::Comma | Token::Semi) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                fields.push(self.parse_type_decl_field()?);
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end type body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::TypeDecl {
            name,
            type_params,
            fields,
        })
    }

    pub(super) fn parse_type_decl_field(&mut self) -> Result<(String, String), String> {
        // V3-18 wedge — `readonly` modifier on a type-body field
        // (`interface X { readonly id: number }`). TS-side only;
        // subset accepts and discards. Detect when followed by an
        // ident-shaped field name.
        if let Token::Ident(s) = self.peek()
            && s == "readonly"
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Ident(_))
        {
            self.pos += 1;
        }
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected field name in type body, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // V3-18 wedge — method-shape field (`m(p: T): R`) in
        // an interface / type-decl body. Per TS spec §3.7 the
        // shape is equivalent to `m: (p: T) => R`. Subset rewrites
        // by parsing the param list + `: R` and synthesizing the
        // arrow-fn type-ann string. Note: type-side only — calls
        // on a struct field that holds a function are not yet
        // lowered, so the wedge unblocks the *parse* of common
        // interface shapes (e.g. matching real class methods via
        // `class C implements I`) even though direct invocation
        // on a struct-typed binding still isn't supported.
        if matches!(self.peek(), Token::LParen) {
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
            let fn_ann =
                crate::type_ann_fnsig::fn_type_ann("__fn", &param_anns.join("|"), &ret_ann);
            return Ok((name, fn_ann));
        }
        // V3-18 wedge — optional field `field?: T` in a `type X = {...}`
        // declaration. Same modeling as the inline-obj path: optional
        // promotes T → __nullable(T) since we don't carry a separate
        // Type::Undefined for absent-vs-null.
        let optional = matches!(self.peek(), Token::Question);
        if optional {
            self.pos += 1;
        }
        match self.peek() {
            Token::Colon => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `:` after field name `{name}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let ty_raw = self.parse_type_ann()?;
        let ty = if optional && !ty_raw.starts_with("__nullable(") {
            format!("__nullable({ty_raw})")
        } else {
            ty_raw
        };
        Ok((name, ty))
    }
}
