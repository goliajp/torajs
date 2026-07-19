//! TypeScript type-annotation parsing (chunk 409).
//!
//! Extracted verbatim from parser.rs:
//! parse_type_ann — top-level type reader (idents, arrays, generics,
//!   inline object literals, string/number/boolean literal types,
//!   `readonly`, `T | null`, `this` polymorphic type, and `is T`
//!   type predicate return);
//! parse_fn_type_ann — the `(params) => Ret` fn-type shape,
//!   delegated to when parse_type_ann sees a `(`.
//!
//! Both methods `pub(super)` for cross-module impl-block access from
//! parser.rs main file and other parser/*.rs siblings (parse_postfix,
//! parse_stmt, parse_class_decl, try_parse_for_of, object_member,
//! parse_class_member_method, parse_class_member_field). Body
//! unchanged.

use super::*;

impl<'a> Parser<'a> {
    /// Recording wrapper (RFC 20260719-fn-tostring-source B2) — the
    /// outermost annotation pushes its byte range into
    /// `ast.type_ann_spans`; recursive inner calls (generic args,
    /// union halves, fn-typed param anns) nest inside that range and
    /// stay unrecorded. `fn_source_erase` splices the recorded
    /// ranges (plus each one's leading `:` / `?` / `as`) out of a fn
    /// span to produce the type-erased source toString answers.
    pub(super) fn parse_type_ann(&mut self) -> Result<String, String> {
        let record = self.type_ann_depth == 0;
        let start_pos = self.pos;
        self.type_ann_depth += 1;
        let r = self.parse_type_ann_inner();
        self.type_ann_depth -= 1;
        if record && r.is_ok() {
            let span = self.span_from(start_pos);
            self.ast.type_ann_spans.push(span);
        }
        r
    }

    fn parse_type_ann_inner(&mut self) -> Result<String, String> {
        // V3-18 wedge — TS type-predicate return type:
        //   function isT(v: any): v is T { ... }
        // Per TS spec §3.6.5 the return type is `boolean` at the
        // value level; the `is T` half is a flow-narrowing hint
        // for callers. The subset accepts and discards the
        // predicate (no flow narrowing) — typecheck sees the
        // function's return as `boolean`. Matched only when the
        // shape is `<paramName> is <Type>`.
        if let Token::Ident(_) = self.peek()
            && let Some(Token::Ident(maybe_is)) = self.tokens.get(self.pos + 1).map(|s| &s.token)
            && maybe_is == "is"
        {
            self.pos += 2; // consume <param> + "is"
            let _ = self.parse_type_ann()?; // consume the asserted type
            return Ok("boolean".to_string());
        }
        // V3-18 wedge — `readonly T[]` modifier on array-of types.
        // Per TS spec §3.10.2 the modifier is type-side and has no
        // runtime effect; the subset treats it as an identity skip.
        // Common in fn-param positions like `xs: readonly number[]`.
        if let Token::Ident(s) = self.peek() {
            if s == "readonly" {
                let next = self.tokens.get(self.pos + 1).map(|t| &t.token);
                if matches!(
                    next,
                    Some(Token::Ident(_))
                        | Some(Token::Void)
                        | Some(Token::LBrace)
                        | Some(Token::LParen)
                ) {
                    self.pos += 1;
                }
            }
        }
        // Function type: `(p: T, ...) => R`.
        if matches!(self.peek(), Token::LParen) {
            return self.parse_fn_type_ann();
        }
        // V3-18 P2.4.c.2 — inline object type literal `{ x: T; y: U }`.
        // Encoded as `__inlobj(x:T|y:U)` for downstream check.rs to
        // decode into a Type::Struct. Same encoding scheme as `__fn(...)`.
        if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut fields: Vec<String> = Vec::new();
            if !matches!(self.peek(), Token::RBrace) {
                loop {
                    // V3-18 wedge — `readonly` modifier on an inline-obj
                    // field. Type-side only; subset accepts and discards.
                    if let Token::Ident(s) = self.peek()
                        && s == "readonly"
                        && let Some(next) = self.tokens.get(self.pos + 1)
                        && matches!(next.token, Token::Ident(_))
                    {
                        self.pos += 1;
                    }
                    let fname = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected field name in inline obj type, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    // V3-18 wedge — optional field `field?: T`. TS spec
                    // §3.9: makes the property absence-tolerant. Subset
                    // models it as `T | null` (we don't have a separate
                    // Type::Undefined for property absence yet).
                    let optional = matches!(self.peek(), Token::Question);
                    if optional {
                        self.pos += 1;
                    }
                    match self.peek() {
                        Token::Colon => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `:` after inline obj field name, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    let fty_raw = self.parse_type_ann()?;
                    // Chunk 793 — struct-field fn slots are
                    // Closure-repr. Retag at birth: this is the site
                    // that mints `__inlobj(` from syntax, and nested
                    // inline objects recurse through here, so every
                    // downstream consumer (checker / parse_type /
                    // collectors / forwarder synthesis) sees the same
                    // `__cls(` field the named-TypeDecl lane sees.
                    let fty_raw = crate::ast::retag_field_fn_ann(&fty_raw);
                    let fty = if optional && !fty_raw.starts_with("__nullable(") {
                        format!("__nullable({fty_raw})")
                    } else {
                        fty_raw
                    };
                    fields.push(format!("{fname}:{fty}"));
                    match self.peek() {
                        Token::Comma | Token::Semi => self.pos += 1,
                        Token::RBrace => break,
                        t => {
                            return Err(format!(
                                "expected `,` `;` or `}}` in inline obj type, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    if matches!(self.peek(), Token::RBrace) {
                        break;
                    }
                }
            }
            match self.peek() {
                Token::RBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `}}` to end inline obj type, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let mut name = format!("__inlobj({})", fields.join("|"));
            // V3-18 wedge — `{ ... } | null` shape. Mirror the
            // post-Ident pipe handler below for the inline-obj case.
            while matches!(self.peek(), Token::LBracket)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|s| &s.token),
                    Some(Token::RBracket)
                )
            {
                self.pos += 2;
                name.push_str("[]");
            }
            if matches!(self.peek(), Token::Pipe) {
                self.pos += 1;
                if matches!(self.peek(), Token::Null) {
                    self.pos += 1;
                    name = format!("__nullable({name})");
                } else {
                    return Err(format!(
                        "only `T | null` unions are supported (no other unions yet); got {:?} at {}",
                        self.peek(),
                        self.at()
                    ));
                }
            }
            return Ok(name);
        }
        // V3-18 wedge — string-literal type-ann (`type Mode =
        // "dev" | "prod"`). Per TS spec §3.2.10 a string literal
        // is a type that has only that literal as its value. The
        // subset widens to plain `string` and consumes any
        // following `| "..."` chain (treating them as further
        // string-literal alternatives that all collapse to the
        // same `string`).
        if let Token::String(_) = self.peek() {
            self.pos += 1;
            while matches!(self.peek(), Token::Pipe)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|s| &s.token),
                    Some(Token::String(_))
                )
            {
                self.pos += 2;
            }
            return Ok("string".to_string());
        }
        // V3-18 wedge — number-literal type-ann (`type Bit =
        // 0 | 1`). Same widening to plain `number`.
        if let Token::Number(_) = self.peek() {
            self.pos += 1;
            while matches!(self.peek(), Token::Pipe)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|s| &s.token),
                    Some(Token::Number(_))
                )
            {
                self.pos += 2;
            }
            return Ok("number".to_string());
        }
        // V3-18 wedge — boolean-literal type-ann (`type Always =
        // true`). Same widening to plain `boolean`.
        if matches!(self.peek(), Token::True | Token::False) {
            self.pos += 1;
            while matches!(self.peek(), Token::Pipe)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|s| &s.token),
                    Some(Token::True) | Some(Token::False)
                )
            {
                self.pos += 2;
            }
            return Ok("boolean".to_string());
        }
        let mut name = match self.peek() {
            Token::Ident(n) => n.clone(),
            // V3-18 m1.h.30 — `void` was promoted to a keyword for
            // the unary operator path, but it's also the canonical
            // return type for void-returning fns: `: void`. Accept
            // it here so type annotations still resolve.
            Token::Void => "void".to_string(),
            // V3-18 wedge — `this` as a type annotation (TS
            // polymorphic-this, spec §3.6.3). Standard in fluent
            // builder APIs:
            //   class Builder { add(...): this { return this } }
            // Parsed as the literal token "this"; desugar_classes
            // rewrites occurrences in a method's return type to
            // the enclosing class's this_ann (cname or
            // cname<TParams>) before emit. Outside class methods
            // the placeholder leaks through to typecheck and fails
            // there — TS only allows `this` types inside class
            // bodies anyway, so this matches the spec.
            Token::This => "this".to_string(),
            t => {
                return Err(format!("expected type name, got {t:?} at {}", self.at()));
            }
        };
        self.pos += 1;
        // M3.4 — generic type instantiation `Pair<A, B>`. Encoded into the
        // flat ann string as `Pair<A|B>` (inner `|` mirrors the `__fn(P|Q)`
        // separator). Same depth-aware decoding shape, so check.rs and
        // ssa_lower can share parsers with the existing fn-type reader.
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            let args = self.parse_type_args_list()?;
            name = format!("{name}<{}>", args.join("|"));
        }
        self.read_type_postfix(name)
    }

    /// Comma-separated type-arg list after an already-consumed `<`,
    /// consuming through the closing `>` (with S155 ShrShr/ShrShrShr
    /// peeling for nested generics). Shared between the generic
    /// type-ann arm above and `parse_primary_new`'s explicit
    /// instantiation (`new Map<string, number>()`).
    pub(super) fn parse_type_args_list(&mut self) -> Result<Vec<String>, String> {
        let mut args: Vec<String> = Vec::new();
        if !matches!(self.peek(), Token::Gt) {
            loop {
                args.push(self.parse_type_ann()?);
                // S155 — ShrShr/ShrShrShr at the close position
                // signals a nested-generic `>>` / `>>>`; treat it
                // as a close marker so the outer close arm can peel
                // the next virtual `>`.
                match self.peek() {
                    Token::Comma => self.pos += 1,
                    Token::Gt => break,
                    _ if matches!(
                        &self.tokens[self.pos].token,
                        Token::ShrShr | Token::ShrShrShr
                    ) =>
                    {
                        break;
                    }
                    t => {
                        return Err(format!(
                            "expected `,` or `>` in type args, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            }
        }
        // S155 — close consumes one virtual `>`. If we're inside
        // `Foo<Bar<X>>` the real token is ShrShr; peel it once and
        // advance pos when fully consumed (peel == 2 for ShrShr,
        // peel == 3 for ShrShrShr).
        match &self.tokens[self.pos].token {
            Token::Gt => {
                self.pos += 1;
                self.type_close_peel = 0;
            }
            Token::ShrShr => {
                self.type_close_peel += 1;
                if self.type_close_peel >= 2 {
                    self.pos += 1;
                    self.type_close_peel = 0;
                }
            }
            Token::ShrShrShr => {
                self.type_close_peel += 1;
                if self.type_close_peel >= 3 {
                    self.pos += 1;
                    self.type_close_peel = 0;
                }
            }
            _ => {
                return Err(format!(
                    "expected `>` to close type args, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
        }
        Ok(args)
    }

    /// Shared postfix readers on a just-parsed type: `[]` array
    /// suffixes and the single-side `| null` nullable wrapper.
    /// Chunk 735 — extracted from the parse_type_ann tail so the
    /// parenthesized-type arm in parse_fn_type_ann reads the same
    /// postfixes on its inner type (`(() => string)[]`).
    fn read_type_postfix(&mut self, mut name: String) -> Result<String, String> {
        while matches!(self.peek(), Token::LBracket) {
            self.pos += 1;
            match self.peek() {
                Token::RBracket => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `]` in array type, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            name.push_str("[]");
        }
        // Trailing `| null` → wrap in a `__nullable(T)` marker. Parser
        // doesn't try to be a full TS union solver — only the
        // single-side-null case is supported in this subset.
        if matches!(self.peek(), Token::Pipe) {
            self.pos += 1;
            if matches!(self.peek(), Token::Null) {
                self.pos += 1;
                name = format!("__nullable({name})");
            } else {
                return Err(format!(
                    "only `T | null` unions are supported (no other unions yet); got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
        }
        Ok(name)
    }

    pub(super) fn parse_fn_type_ann(&mut self) -> Result<String, String> {
        // current token = `(`
        self.pos += 1;
        let mut params: Vec<String> = Vec::new();
        // Chunk 735 — a `name:` label or rest param pins the shape as
        // a fn-type; without either, a single parenthesized type can
        // re-read as TS ParenthesizedType when no `=>` follows the
        // close paren (`(() => string)[]`).
        let mut fn_shape_pinned = false;
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // `...name: E[]` rest param (RFC 20260708-variadic) —
                // TS grammar: must be last, must be an array type.
                // Encodes as `__rest(E[])` in the param slot.
                if matches!(self.peek(), Token::DotDotDot) {
                    self.pos += 1;
                    fn_shape_pinned = true;
                    let name_then_colon = matches!(self.peek(), Token::Ident(_))
                        && matches!(
                            self.tokens.get(self.pos + 1).map(|s| &s.token),
                            Some(Token::Colon)
                        );
                    if name_then_colon {
                        self.pos += 2;
                    }
                    let pty = self.parse_type_ann()?;
                    if !(pty.ends_with("[]") || (pty.starts_with("Array<") && pty.ends_with('>'))) {
                        return Err(format!(
                            "rest param type must be an array type, got `{pty}` at {}",
                            self.at()
                        ));
                    }
                    params.push(format!("__rest({pty})"));
                    match self.peek() {
                        Token::RParen => break,
                        t => {
                            return Err(format!(
                                "rest param must be last in fn-type params, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
                // Optional `name:` prefix on each param. Name is discarded;
                // we keep only the type. Two shapes accepted:
                //   `name: T` — TS standard fn-type form.
                //   `T`       — bare type, no name (fallback).
                let name_then_colon = matches!(self.peek(), Token::Ident(_))
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|s| &s.token),
                        Some(Token::Colon)
                    );
                if name_then_colon {
                    self.pos += 2;
                    fn_shape_pinned = true;
                }
                let pty = self.parse_type_ann()?;
                params.push(pty);
                match self.peek() {
                    Token::Comma => self.pos += 1,
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in fn-type params, got {t:?} at {}",
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
        match self.peek() {
            Token::FatArrow => self.pos += 1,
            t => {
                // Chunk 735 — TS ParenthesizedType: `(T)` with no
                // `=>` after the close paren is a grouped type, most
                // commonly a fn-type array `(() => string)[]`. Only a
                // single bare type re-reads this way — a `name:`
                // label or rest param pinned the fn-type shape and
                // keeps the loud error.
                if params.len() == 1 && !fn_shape_pinned {
                    let inner = params.pop().expect("single grouped type");
                    return self.read_type_postfix(inner);
                }
                return Err(format!(
                    "expected `=>` in fn-type, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let ret = self.parse_type_ann()?;
        Ok(format!("__fn({})->{}", params.join("|"), ret))
    }
}
