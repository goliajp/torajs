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
use crate::ast::{AccessorKind, Visibility};
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
            | Token::InstanceOf
            | Token::Try
            | Token::Yield
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
            if matches!(name_tok, Some(Token::Ident(_)))
                && matches!(after_name, Some(Token::LParen))
            {
                accessor_kind = Some(match kw.as_str() {
                    "get" => AccessorKind::Getter,
                    _ => AccessorKind::Setter,
                });
                self.pos += 1; // consume `get` / `set`
            }
        }

        Ok(ClassMemberModifierPrefix {
            explicit_visibility,
            is_readonly,
            is_abstract_method,
            is_static,
            accessor_kind,
        })
    }
}
