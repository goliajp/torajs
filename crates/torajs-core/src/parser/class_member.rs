//! Class-member modifier-prefix parsing.
//!
//! Extracted from `parser.rs::parse_class_decl_with_abstract` (2026-06-03,
//! god-file decomp — P10.3-A3a prerequisite per `file-size.md` "先拆再加"
//! rule). The modifier prefix `[visibility] [readonly] [abstract] [static]
//! [get|set]` precedes every class member name (field / method / accessor)
//! and is fully shape-driven from token lookahead — no AST-arena mutation,
//! pure consumption of `self.tokens` / `self.pos`. Pulling it out lets
//! `parse_class_decl_with_abstract` drop ~200 LOC, and gives P10.3-A3a a
//! clean place to add `async` modifier recognition without touching the
//! god-fn.
//!
//! Parser-internal `pub(super)` API:
//!   * `ClassMemberModifierPrefix` — parsed prefix payload.
//!   * `Parser::parse_class_member_modifier_prefix` — consumes the prefix
//!     tokens and returns the payload, or an Err for cross-modifier
//!     conflicts (`static abstract`, abstract in non-abstract class).

use super::Parser;
use crate::ast::{AccessorKind, ClassMethod, Param, Stmt, Visibility};
use crate::lexer::Token;

/// One class member's modifier prefix. `accessor_kind` may still be
/// mutated by the caller along the PrivateIdent path (see god-fn 5970
/// region) where `private` mangling is applied post-prefix.
pub(super) struct ClassMemberModifierPrefix {
    pub explicit_visibility: Option<Visibility>,
    pub is_readonly: bool,
    pub is_abstract_method: bool,
    pub is_static: bool,
    pub accessor_kind: Option<AccessorKind>,
    /// P10.3-A3a — `async` method modifier (`[static] async name(...) {}`).
    /// When true `finalize_class_method` registers the mangled
    /// `__cm_<C>__<M>` / `__sm_<C>__<M>` fn name into `ast.async_fns`
    /// so `desugar_async` picks it up.
    pub is_async: bool,
    /// RFC 20260719-fn-tostring-source B3a — byte offset of the
    /// consumed `async` / `get` / `set` modifier token when one was
    /// eaten by the prefix parse. The MethodDefinition source span
    /// starts there (ES §20.2.3.5); `None` means the span starts at
    /// the member name, which the caller still holds.
    pub member_span_start: Option<u32>,
}

/// Tokens that may legally start a class member name. Used by the
/// visibility / abstract / static lookahead to distinguish a real
/// modifier from a same-spelled member name. The TS-keyword variants
/// (`return`, `throw`, ...) are included per ES spec §12.7.6
/// (PropertyName allows IdentifierName which includes reserved words).
/// `allow_private` widens the set to include `#x` PrivateIdent — only
/// the `static` modifier accepts that (handled in field-decl dispatch
/// downstream with a targeted "static private fields not yet supported"
/// error rather than silently consuming `static` as a field name).
fn token_starts_class_member_name(t: Option<&Token>, allow_private: bool) -> bool {
    let Some(t) = t else { return false };
    if allow_private && matches!(t, Token::PrivateIdent(_)) {
        return true;
    }
    matches!(
        t,
        Token::Ident(_)
            | Token::Catch
            | Token::Finally
            | Token::Return
            | Token::Throw
            | Token::If
            | Token::Else
            | Token::For
            | Token::While
            | Token::Do
            | Token::Break
            | Token::Continue
            | Token::Switch
            | Token::Case
            | Token::Default
            | Token::Class
            | Token::New
            | Token::This
            | Token::Function
            | Token::TypeOf
            | Token::Delete
            | Token::InstanceOf
            | Token::Try
            | Token::Yield
            // P10.3-A3a — `static async name(...) {}` / `async name(...) {}`
            // — `Token::Async` is a modifier keyword that legally
            // follows `static`, so the `static` lookahead must accept
            // it here too. The downstream async-modifier check
            // distinguishes the modifier form from `async` as a
            // member name via the trailing `Ident + LParen` lookahead.
            | Token::Async
            // P-SURF S2.1 — `static *g() {}`. `*` legally follows
            // `static`, so without this the `static` lookahead fails and
            // `static` is taken for the member name itself, reporting
            // "expected `(` (method) or `:` (field) after `static`".
            | Token::Star
            // RFC 20260802-class-computed-member 刀 1 — §12.7.6
            // PropertyName also admits string / numeric literals
            // (`get 'default'()` / `static 0x10() {}`) and the
            // ComputedPropertyName `[expr]` form (`static [k]()`).
            | Token::String(_)
            | Token::Number(_)
            | Token::LBracket
    )
}

impl<'a> Parser<'a> {
    /// M-OO.5 / M-OO.6 / P8.1 / P8.2 — parse the `[visibility] [readonly]
    /// [abstract] [static] [get|set]` modifier prefix in front of a class
    /// member. Returns the parsed prefix or an Err for cross-modifier
    /// conflicts. `class_name` is for err messages; `is_abstract_class`
    /// is the enclosing class's `abstract` flag (required for the
    /// "abstract method only allowed in abstract class" check).
    ///
    /// Pure token consumer — does not touch `self.ast`.
    pub(super) fn parse_class_member_modifier_prefix(
        &mut self,
        class_name: &str,
        is_abstract_class: bool,
    ) -> Result<ClassMemberModifierPrefix, String> {
        // M-OO.5 — visibility / readonly modifiers (contextual keywords).
        // Order in TS: `[public|private|protected] [static] [readonly]
        // [abstract] memberName`. We accept them in any order before the
        // abstract / static keywords already handled below — TS's tsc
        // actually requires a specific order, but matching the strict
        // ordering matters less than recognizing the modifiers.
        let mut explicit_visibility: Option<Visibility> = None;
        let mut is_readonly = false;
        loop {
            let Token::Ident(s) = self.peek() else {
                break;
            };
            let candidate = match s.as_str() {
                "public" => Some(Visibility::Public),
                "private" => Some(Visibility::Private),
                "protected" => Some(Visibility::Protected),
                _ => None,
            };
            if let Some(vis) = candidate {
                if explicit_visibility.is_some() {
                    return Err(format!(
                        "duplicate visibility modifier in class `{class_name}` at {}",
                        self.at()
                    ));
                }
                // Lookahead must be a member-name shape — otherwise the
                // ident is being used as a regular member (e.g. `private`
                // as a field name in lax JS).
                let next = self.tokens.get(self.pos + 1).map(|t| &t.token);
                if !token_starts_class_member_name(next, false) {
                    break;
                }
                self.pos += 1;
                explicit_visibility = Some(vis);
                continue;
            }
            if s == "readonly" {
                let next = self.tokens.get(self.pos + 1).map(|t| &t.token);
                if !matches!(next, Some(Token::Ident(_))) {
                    break;
                }
                self.pos += 1;
                is_readonly = true;
                continue;
            }
            break;
        }

        // M-OO.6 — `abstract methodName(...);` (no body). Contextual
        // keyword; skip so the rest of the member-name dispatch reads
        // the actual method name. `static abstract` / abstract in a
        // non-abstract class are rejected after the static lookahead.
        let mut is_abstract_method = false;
        if let Token::Ident(s) = self.peek()
            && s == "abstract"
        {
            let next = self.tokens.get(self.pos + 1).map(|t| &t.token);
            if token_starts_class_member_name(next, false) {
                self.pos += 1;
                is_abstract_method = true;
            }
        }

        // `static <name>` — `static #x` accepted here so the field-decl
        // dispatch can reject it with a targeted error rather than
        // leaving `static` to be parsed as a member name.
        let is_static = if let Token::Ident(s) = self.peek()
            && s == "static"
        {
            let next = self.tokens.get(self.pos + 1).map(|t| &t.token);
            if token_starts_class_member_name(next, true) {
                self.pos += 1;
                true
            } else {
                false
            }
        } else {
            false
        };
        if is_abstract_method && is_static {
            return Err(format!(
                "`static abstract` is not allowed in class `{class_name}` at {}",
                self.at()
            ));
        }
        if is_abstract_method && !is_abstract_class {
            return Err(format!(
                "abstract method only allowed in `abstract class` (class `{class_name}`) at {}",
                self.at()
            ));
        }

        // P10.3-A3a — `[static] async name(...) {}`. Distinguished
        // from `async` as a regular method name by `Ident + LParen`
        // lookahead after the keyword. `abstract async` is a TS
        // error ("'async' modifier cannot be used with 'abstract'");
        // call site rejects the same combo for consistency.
        // P-SURF S2.18 — `async *g() {}` puts a `*` between the modifier
        // and the name, so the lookahead skips over one when it is
        // there. The `*` itself is deliberately left unconsumed: the
        // caller's generator arm is what claims it, and it needs to see
        // it to fire.
        let mut member_span_start: Option<u32> = None;
        let async_star = matches!(self.peek(), Token::Async)
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(Token::Star)
            );
        let name_off = if async_star { 2 } else { 1 };
        // RFC 20260802-class-computed-member 残尾 — §13.4 PropertyName
        // widens the async-method name slot exactly like the get/set
        // arm below: string / numeric literals (`async 'm'()` /
        // `async 0x10()`) keep the trailing-LParen disambiguation, and
        // a `[` head is unambiguous on its own (a member NAMED `async`
        // is always `async(` — never `async [`), so no after-name
        // lookahead applies to the computed form.
        let is_async = if matches!(self.peek(), Token::Async)
            && let Some(t1) = self.tokens.get(self.pos + name_off)
            && (matches!(t1.token, Token::LBracket)
                || (matches!(
                    t1.token,
                    Token::Ident(_) | Token::PrivateIdent(_) | Token::String(_) | Token::Number(_)
                ) && matches!(
                    self.tokens.get(self.pos + name_off + 1).map(|t| &t.token),
                    Some(Token::LParen)
                ))) {
            member_span_start = self.tokens.get(self.pos).map(|t| t.span.start);
            self.pos += 1;
            true
        } else {
            false
        };
        if is_async && is_abstract_method {
            return Err(format!(
                "`async` modifier cannot be used with `abstract` modifier in class `{class_name}` at {}",
                self.at()
            ));
        }

        // P8.2 — accessor descriptor: `get X(): T { ... }` / `set X(v: T)
        // { ... }`. The lexer emits `get` / `set` as `Token::Ident("get"
        // | "set")` (contextual keywords per ES §13.4); recognise here
        // so the property name + body falls through the existing method
        // path with `accessor_kind` tagged on the resulting
        // `ClassMethod`. Lookahead requires Ident name slot + LParen
        // after it so `class C { get(): T { } }` (member named "get")
        // still works. Static accessors (`static get X`) go through the
        // is_static lookahead above.
        let mut accessor_kind: Option<AccessorKind> = None;
        if let Token::Ident(s) = self.peek()
            && (s == "get" || s == "set")
        {
            let kw = s.clone();
            let name_tok = self.tokens.get(self.pos + 1).map(|t| &t.token);
            let after_name = self.tokens.get(self.pos + 2).map(|t| &t.token);
            // S2.37 — `get #p()` / `set #p(v)`: a PrivateIdent is a
            // legal accessor name (§15.4 ClassElementName includes
            // PrivateIdentifier). No ambiguity with a member NAMED
            // `get`: that shape is `get(` — never `get #p(`.
            //
            // RFC 20260802-class-computed-member 刀 1 — §13.4
            // PropertyName widens the accessor-name slot to string /
            // numeric literals (`get 'default'()` / `get 0x10()`) and
            // the ComputedPropertyName `[expr]` head (`get [k]()`).
            // A `[` after the keyword is unambiguous on its own — a
            // member NAMED `get` is always `get(` — so no after-name
            // lookahead applies (the key is an arbitrary expression).
            if matches!(
                name_tok,
                Some(
                    Token::Ident(_) | Token::PrivateIdent(_) | Token::String(_) | Token::Number(_)
                )
            ) && matches!(after_name, Some(Token::LParen))
                || matches!(name_tok, Some(Token::LBracket))
            {
                accessor_kind = Some(match kw.as_str() {
                    "get" => AccessorKind::Getter,
                    _ => AccessorKind::Setter,
                });
                if member_span_start.is_none() {
                    member_span_start = self.tokens.get(self.pos).map(|t| t.span.start);
                }
                self.pos += 1; // consume `get` / `set`
            }
        }

        Ok(ClassMemberModifierPrefix {
            explicit_visibility,
            is_readonly,
            is_abstract_method,
            is_static,
            accessor_kind,
            is_async,
            member_span_start,
        })
    }

    /// Finalize a non-ctor class method after its params / return-type
    /// / body have been parsed. Applies the readonly cross-modifier
    /// check, resolves visibility (default `Public`), registers
    /// `member_visibility` for non-default values, registers async
    /// method mangled names into `ast.async_fns` (P10.3-A3a), and
    /// pushes the `ClassMethod` onto the appropriate methods or
    /// static_methods vector. Centralised so the god-fn doesn't grow
    /// when new modifier-driven side-effects (async / generator /
    /// override / ...) are added later.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_class_method(
        &mut self,
        class_name: &str,
        member_name: String,
        type_params: Vec<String>,
        params: Vec<Param>,
        return_type: Option<String>,
        body: Vec<Stmt>,
        explicit_visibility: Option<Visibility>,
        accessor_kind: Option<AccessorKind>,
        is_readonly: bool,
        is_abstract_method: bool,
        is_static: bool,
        is_async: bool,
        span: crate::lexer::Span,
        methods: &mut Vec<ClassMethod>,
        static_methods: &mut Vec<ClassMethod>,
    ) -> Result<(), String> {
        if is_readonly {
            return Err(format!(
                "`readonly` modifier is only valid on fields, not on method `{member_name}` in class `{class_name}` at {}",
                self.at()
            ));
        }
        let visibility = explicit_visibility.unwrap_or(Visibility::Public);
        if visibility != Visibility::Public {
            self.ast
                .member_visibility
                .insert((class_name.to_string(), member_name.clone()), visibility);
        }
        if is_async {
            let mangled = if is_static {
                format!("__sm_{class_name}__{member_name}")
            } else {
                format!("__cm_{class_name}__{member_name}")
            };
            self.ast.async_fns.insert(mangled);
        }
        let m = ClassMethod {
            name: member_name,
            type_params,
            params,
            return_type,
            body,
            is_abstract: is_abstract_method,
            visibility,
            accessor_kind,
            span,
        };
        if is_static {
            static_methods.push(m);
        } else {
            methods.push(m);
        }
        Ok(())
    }
}
