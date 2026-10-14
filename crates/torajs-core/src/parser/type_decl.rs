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

mod method_shape;

use super::*;

impl<'a> Parser<'a> {
    /// V3-18 wedge — `interface X { ... }` parsing. Per TS spec
    /// §3.7, interfaces are nominal type-side declarations; the
    /// subset treats them as alias for `type X = { ... }` (no
    /// declaration-merging / heritage clauses are honored beyond
    /// what's already covered by `type`).
    pub(super) fn parse_interface_decl(&mut self) -> Result<Stmt, String> {
        self.with_literal_types(Self::parse_interface_decl_body)
    }

    fn parse_interface_decl_body(&mut self) -> Result<Stmt, String> {
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
        let type_params = self.parse_decl_type_params("interface")?;
        // `extends Foo, Bar<T>` — each heritage type rides as a leading
        // field row that `ast::flatten_interface_extends` expands into
        // the parent's members.
        let mut fields: Vec<(PropKey, String)> = Vec::new();
        if matches!(self.peek(), Token::Extends) {
            self.pos += 1;
            loop {
                let parent = self.parse_type_ann()?;
                fields.push((
                    PropKey::from(crate::ast::interface_extends::EXTENDS_FIELD),
                    parent,
                ));
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
        }
        if matches!(self.peek(), Token::LBrace) && self.brace_body_has_index_sig() {
            return self.finish_dict_interface(name, type_params, fields);
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
        self.parse_type_decl_fields(&name, &mut fields)?;
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
        self.with_literal_types(Self::parse_type_decl_body)
    }

    fn parse_type_decl_body(&mut self) -> Result<Stmt, String> {
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
        let type_params = self.parse_decl_type_params("type")?;
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
        if !matches!(self.peek(), Token::LBrace)
            || self.brace_type_heads_union()
            || self.brace_body_has_index_sig()
        {
            let ann = self.parse_type_ann()?;
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
            }
            return Ok(Stmt::TypeDecl {
                name,
                type_params,
                fields: vec![(PropKey::from("__alias__"), ann)],
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
        let mut fields: Vec<(PropKey, String)> = Vec::new();
        self.parse_type_decl_fields(&name, &mut fields)?;
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

    /// The optional `<A, B>` type-parameter list of a `type` /
    /// `interface` declaration (`what` names it in errors).
    fn parse_decl_type_params(&mut self, what: &str) -> Result<Vec<String>, String> {
        let mut type_params: Vec<String> = Vec::new();
        if !matches!(self.peek(), Token::Lt) {
            return Ok(type_params);
        }
        self.pos += 1;
        while !matches!(self.peek(), Token::Gt) {
            match self.peek() {
                Token::Ident(n) => {
                    type_params.push(n.clone());
                    self.pos += 1;
                }
                t => {
                    return Err(format!(
                        "expected type-parameter name in {what}<...>, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            match self.peek() {
                Token::Comma => self.pos += 1,
                Token::Gt => break,
                t => {
                    return Err(format!(
                        "expected `,` or `>` in {what} type params, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        self.pos += 1;
        Ok(type_params)
    }

    /// The members of a declaration body, up to (not consuming) its
    /// `}`. A member ends at `,`, `;` or a line break (TS §3.9: the
    /// separator of a type member list is optional before a newline).
    fn parse_type_decl_fields(
        &mut self,
        owner: &str,
        fields: &mut Vec<(PropKey, String)>,
    ) -> Result<(), String> {
        while !matches!(self.peek(), Token::RBrace) {
            fields.push(self.parse_type_decl_field(owner)?);
            if matches!(self.peek(), Token::Comma | Token::Semi) {
                self.pos += 1;
            } else if !self.has_newline_before(self.pos) {
                break;
            }
        }
        Ok(())
    }

    pub(super) fn parse_type_decl_field(
        &mut self,
        owner: &str,
    ) -> Result<(PropKey, String), String> {
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
        if matches!(self.peek(), Token::LParen) {
            return Ok((PropKey::from(name), self.parse_method_shape_ann()?));
        }
        // V3-18 wedge — optional field `field?: T` in a `type X = {...}`
        // declaration. Same modeling as the inline-obj path: optional
        // promotes T → __nullable(T) since we don't carry a separate
        // Type::Undefined for absent-vs-null.
        let optional = matches!(self.peek(), Token::Question);
        if optional {
            self.pos += 1;
            let at = (owner.to_string(), PropKey::from(name.as_str()));
            self.ast.optional_members.all.insert(at);
        }
        // `m?(p: T): R` — an optional method is an optional member of
        // the method's fn type
        if optional && matches!(self.peek(), Token::LParen) {
            let ann = self.parse_method_shape_ann()?;
            return Ok((PropKey::from(name), format!("__nullable({ann})")));
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
            let at = (owner.to_string(), PropKey::from(name.as_str()));
            self.ast.optional_members.plain.insert(at);
            format!("__nullable({ty_raw})")
        } else {
            ty_raw
        };
        Ok((PropKey::from(name), ty))
    }
}
