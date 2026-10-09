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

mod inline_obj;
mod predicate;
mod union_chain;
pub(crate) use union_chain::union_combine;

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
        // a union may open with a `|` of its own (TS §3.4), the
        // spelling that puts each member on a line of its own
        if matches!(self.peek(), Token::Pipe) {
            self.pos += 1;
        }
        self.type_ann_depth += 1;
        let r = self.parse_type_ann_inner();
        self.type_ann_depth -= 1;
        if record && r.is_ok() {
            let span = self.span_from(start_pos);
            self.ast.type_ann_spans.push(span);
        }
        r
    }

    pub(super) fn parse_type_ann_inner(&mut self) -> Result<String, String> {
        if let Some(pred) = self.parse_predicate_sig() {
            return pred;
        }
        // V3-18 wedge — `readonly T[]` modifier on array-of types.
        // Per TS spec §3.10.2 the modifier is type-side and has no
        // runtime effect; the subset treats it as an identity skip.
        // Common in fn-param positions like `xs: readonly number[]`.
        if let Token::Ident(s) = self.peek()
            && s == "readonly"
        {
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
        // Function type: `(p: T, ...) => R`.
        if matches!(self.peek(), Token::LParen) {
            return self.parse_fn_type_ann();
        }
        // Object type literal `{ x: T }` / `{ [k: string]: V }`.
        if matches!(self.peek(), Token::LBrace) {
            return self.parse_inline_obj_type();
        }
        // V3-18 wedge — tuple type ann (`[number, string]`, TS spec
        // §3.3.3). The subset has no fixed-arity array type, so a
        // tuple widens to its element array: `T[]` when every member
        // spells the same `T`, `any[]` otherwise — the same widening
        // posture as the literal-type wedges below (a tuple IS an
        // array; reads come back as the widened element). Optional
        // (`T?`) and rest (`...T`) members keep the loud reject
        // until a real need shows up.
        if matches!(self.peek(), Token::LBracket) {
            self.pos += 1;
            let mut elems: Vec<String> = Vec::new();
            if !matches!(self.peek(), Token::RBracket) {
                loop {
                    elems.push(self.parse_type_ann_inner()?);
                    match self.peek() {
                        Token::Comma => {
                            self.pos += 1;
                            if matches!(self.peek(), Token::RBracket) {
                                break;
                            }
                        }
                        Token::RBracket => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `]` in tuple type, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
            }
            match self.peek() {
                Token::RBracket => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `]` to end tuple type, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let elem = match elems.first() {
                Some(first) if elems.iter().all(|e| e == first) => first.clone(),
                _ => "any".to_string(),
            };
            let mut name = format!("{elem}[]");
            // Mirror the inline-obj tail: `[T, U][]` / `| null`.
            while matches!(self.peek(), Token::LBracket)
                && matches!(
                    self.tokens.get(self.pos + 1).map(|s| &s.token),
                    Some(Token::RBracket)
                )
            {
                self.pos += 2;
                name.push_str("[]");
            }
            name = self.consume_union_suffix(name)?;
            return Ok(name);
        }
        if let Some(widened) = self.literal_type_widen() {
            return self.read_type_postfix(widened);
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
            // `null` as a type annotation — TS's null type, whose only
            // value is `null`. It lexes as a keyword, not an Ident, so
            // it never reached the arm above, while `undefined` (an
            // Ident) has always resolved. Both ends of the pipeline
            // were already waiting for the spelling: `check_type_ann`
            // answers `Type::Null` for it and `ssa_lower_parse_type`
            // answers the null-shaped pointer. Only the parser could
            // not say the word.
            Token::Null => "null".to_string(),
            t => {
                return Err(format!("expected type name, got {t:?} at {}", self.at()));
            }
        };
        name = self.enum_type_ref(name);
        // M3.4 — generic type instantiation `Pair<A, B>`. Encoded into the
        // flat ann string as `Pair<A|B>` (inner `|` mirrors the `__fn(P|Q)`
        // separator). Same depth-aware decoding shape, so check.rs and
        // ssa_lower can share parsers with the existing fn-type reader.
        if matches!(self.peek(), Token::Lt) {
            self.pos += 1;
            let args_pos = self.pos;
            let args = self.parse_type_args_list()?;
            name = match self.record_dict_ann(&name, &args, args_pos) {
                Some(dict) => dict,
                None => format!("{name}<{}>", args.join("|")),
            };
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
        self.consume_type_close("type args")?;
        Ok(args)
    }

    /// Consume one virtual `>` closing a type's angle brackets. S155:
    /// inside `Foo<Bar<X>>` the real token is ShrShr; peel it once and
    /// advance pos when fully consumed (peel == 2 for ShrShr, peel == 3
    /// for ShrShrShr).
    pub(super) fn consume_type_close(&mut self, what: &str) -> Result<(), String> {
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
                    "expected `>` to close {what}, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
        }
        Ok(())
    }

    /// Shared postfix readers on a just-parsed type: `[]` array
    /// suffixes and a trailing union chain.
    /// Chunk 735 — extracted from the parse_type_ann tail so the
    /// parenthesized-type arm in parse_fn_type_ann reads the same
    /// postfixes on its inner type (`(() => string)[]`).
    /// An array suffix does not continue past a line break (TS reads
    /// `T` then a new member at `T\n[k: string]: V`).
    pub(super) fn read_type_postfix(&mut self, mut name: String) -> Result<String, String> {
        while matches!(self.peek(), Token::LBracket) && !self.has_newline_before(self.pos) {
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
        name = self.consume_union_suffix(name)?;
        Ok(name)
    }
}
