//! `Parser::parse_class_decl_with_abstract` extracted from
//! `parser.rs` (chunk 165). Largest single parser god-fn.
//!
//! Pre-extract this method was 638 LOC inside `impl Parser` block.
//! Body verbatim moves here as impl-block sibling (same pattern as
//! chunks 162/163/164's parse_stmt / try_parse_for_of /
//! parse_postfix extractions).
//!
//! `parse_class_decl_with_abstract` parses `[abstract] class C
//! [extends P]<T...> { fields, ctor, methods, static_methods }`
//! including abstract modifier, generic params, extends parent,
//! field decls, constructor (special-cased), instance methods,
//! static methods + static field initializers (StaticInit::Block
//! for static blocks), accessor getters/setters (P8.2 — separate
//! AccessorKind).
//!
//! Body unchanged.
//!
//! 2026-07-03 fn-debt decomp: header trio (name / type params /
//! heritage + body-open) → `parse_class_decl_header.rs`; static
//! block / member-name / untyped-field / field-init finalize →
//! `parse_class_decl_member.rs`.

use super::*;

use super::parse_class_decl_header::ClassHeader;

impl<'a> Parser<'a> {
    pub(super) fn parse_class_decl_with_abstract(
        &mut self,
        is_abstract: bool,
        allow_anon: bool,
        force_synth: bool,
    ) -> Result<Stmt, String> {
        // 420-06 — the cursor sits at `class`; the byte range through
        // the body's closing brace feeds the §20.2.3.5 class-ctor
        // toString source (an `abstract` prefix stays outside — the
        // erased text starts at `class`, which is the shape the
        // runtime answers anyway).
        let class_start = self.pos;
        let h = self.parse_class_header(allow_anon, force_synth)?;
        let ClassHeader {
            name,
            type_params,
            parent,
            parent_name,
            saved_class,
            saved_super_prop,
            saved_has_parent,
        } = h;
        let mut fields: Vec<(PropKey, String)> = Vec::new();
        let mut static_init: Vec<StaticInit> = Vec::new();
        let mut ctor: Option<ClassCtor> = None;
        let mut methods: Vec<ClassMethod> = Vec::new();
        let mut static_methods: Vec<ClassMethod> = Vec::new();
        // V3-18 wedge — instance-field initializers (`val: T = init`).
        // Collected here in source order; appended to the ctor body
        // (a synthesized one if no ctor was declared) at class-decl
        // finalization. The synthesized prefix is "this.<n> = init"
        // per declared field.
        let mut field_inits: Vec<(PropKey, ExprId)> = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Each member is one of:
            //   - `constructor(params) { body }`
            //   - `methodName(params): R? { body }`
            //   - `fieldName: T;`                       (instance field)
            //   - `static methodName(params): R? { body }`  (M-OO.4)
            //   - `static fieldName: T = init;`              (M-OO.4)
            //   - `static { stmts; }`                        (P8.3-A2; ES2022 §15.7.10)
            // We disambiguate by lookahead: ident then `(` ⇒ ctor or method;
            // ident then `:` ⇒ field declaration. The `static` modifier is a
            // contextual keyword: only treated as such when the next token
            // is a valid member name shape.

            // S2.40 — `ClassElement : ;` (ES §15.7): a bare semicolon
            // is an empty class element. The t262 elements/ suites
            // end every class body with one (474-case `got Semi`
            // parse wall).
            if matches!(self.peek(), Token::Semi) {
                self.pos += 1;
                continue;
            }

            // P8.3-A2 — `static { ... }` class static block (ES2022 §15.7.10).
            // Detected at the top of each iteration, before modifier parsing,
            // because the `is_static`-modifier dispatch below assumes `static`
            // precedes a member NAME, not a block body. Visibility / readonly
            // / abstract are not valid on static blocks per spec, so refusing
            // to consume them above the block is correct — `public static {}`
            // and similar fall through to the existing modifier-misapplication
            // error.
            if let Token::Ident(s) = self.peek()
                && s == "static"
                && matches!(
                    self.tokens.get(self.pos + 1).map(|t| &t.token),
                    Some(Token::LBrace)
                )
            {
                self.parse_class_static_block(&name, &mut static_init)?;
                continue;
            }

            // Modifier prefix — see `parser/class_member.rs`.
            let ClassMemberModifierPrefix {
                mut explicit_visibility,
                is_readonly,
                is_abstract_method,
                is_static,
                accessor_kind,
                is_async,
                member_span_start,
            } = self.parse_class_member_modifier_prefix(&name, is_abstract)?;
            // RFC 20260719-fn-tostring-source B3a — the MethodDefinition
            // span starts at the consumed `async`/`get`/`set` modifier
            // or, failing that, the member-name token now under `pos`
            // (captured before `parse_class_member_name` consumes it).
            let member_span_start = member_span_start
                .or_else(|| self.tokens.get(self.pos).map(|t| t.span.start))
                .unwrap_or(0);
            // P-SURF S2.1 — `*g() { yield 1 }`. Checked before member-name
            // parsing, which is where `*` used to die with `expected class
            // member name, got Star`. See
            // `parser/parse_class_decl_generator.rs`: the method is hoisted
            // to a top-level `function*` taking the receiver as a parameter
            // and the class keeps an ordinary forwarder, so nothing
            // downstream of here changes.
            if matches!(self.peek(), Token::Star) {
                let visibility = explicit_visibility.unwrap_or(ast::Visibility::Public);
                self.parse_class_generator_method(
                    &name,
                    parent_name.as_deref(),
                    is_static,
                    is_async,
                    visibility,
                    member_span_start,
                    &mut methods,
                    &mut static_methods,
                )?;
                continue;
            }
            // Computed / private / reserved-word member-name parsing
            // — split to `parse_class_decl_member.rs`.
            let (member_name, consumed_computed_name) =
                self.parse_class_member_name(&name, is_static, &mut explicit_visibility)?;
            let next_tok = if consumed_computed_name {
                // We already consumed name + `]`, so the next token is
                // the one driving the member-shape decision (LParen
                // for method, Colon for field, Eq for typed-field).
                self.tokens.get(self.pos).map(|s| &s.token)
            } else {
                self.tokens.get(self.pos + 1).map(|s| &s.token)
            };
            match next_tok {
                // ctor or method (sub-sibling, chunk 174); `Lt` opens a
                // method's own type-parameter list (398-01). Ok(true) =
                // TS overload signature, skip + continue outer loop.
                Some(Token::LParen) | Some(Token::Lt) => {
                    if self.parse_class_member_method_or_ctor(
                        &name,
                        member_name,
                        consumed_computed_name,
                        explicit_visibility,
                        is_readonly,
                        is_abstract_method,
                        is_static,
                        is_async,
                        accessor_kind,
                        member_span_start,
                        &mut fields,
                        &mut ctor,
                        &mut methods,
                        &mut static_methods,
                    )? {
                        continue;
                    }
                }
                // §9.2 optional field — `p?: T`, `p?`, `p? = init`;
                // and 563-07 its TS twin `p!: T`, the definite
                // assignment assertion. Both shift every downstream
                // cursor by one and differ only in whether the
                // declared type widens — see `FieldMarker`.
                Some(Token::Question) | Some(Token::Bang) => {
                    let marker = if matches!(next_tok, Some(Token::Bang)) {
                        FieldMarker::Definite
                    } else {
                        FieldMarker::Optional
                    };
                    self.parse_class_member_field_dispatch(
                        &name,
                        member_name,
                        consumed_computed_name,
                        marker,
                        explicit_visibility,
                        is_readonly,
                        is_abstract_method,
                        is_static,
                        &mut fields,
                        &mut static_init,
                        &mut field_inits,
                    )?;
                }
                Some(Token::Colon) | Some(Token::Eq) | Some(Token::Semi) | Some(Token::RBrace) => {
                    // Field declaration — typed (`:`), initialized
                    // (`=`), or bare (`;` / trailing `}`). One
                    // shared-surface dispatch in
                    // `parse_class_decl_member.rs`.
                    self.parse_class_member_field_dispatch(
                        &name,
                        member_name,
                        consumed_computed_name,
                        FieldMarker::None,
                        explicit_visibility,
                        is_readonly,
                        is_abstract_method,
                        is_static,
                        &mut fields,
                        &mut static_init,
                        &mut field_inits,
                    )?;
                }
                t => {
                    // ES §12.10 ASI — a bare FieldDefinition's `;` can
                    // be supplied by a line break: `a` on its own line
                    // followed by the next member (`*m() {} a\nb = 42`,
                    // the t262 after-same-line-*-asi family). Only a
                    // NEWLINE triggers it (`a b` on one line stays the
                    // loud error), and only for a plain unconsumed
                    // name (a computed key's shape token was already
                    // decided above).
                    if !consumed_computed_name && self.has_newline_before(self.pos + 1) {
                        self.parse_class_member_field_dispatch(
                            &name,
                            member_name,
                            consumed_computed_name,
                            FieldMarker::None,
                            explicit_visibility,
                            is_readonly,
                            is_abstract_method,
                            is_static,
                            &mut fields,
                            &mut static_init,
                            &mut field_inits,
                        )?;
                        continue;
                    }
                    return Err(self.member_shape_err(&member_name, t));
                }
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end class body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // Span recorded BEFORE the trailing `;` consume — the source
        // text ends at the body's closing brace.
        let class_span = self.span_from(class_start);
        self.ast.class_decl_spans.insert(name.clone(), class_span);
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        // V3-18 wedge — prepend `this.<n> = <init>` stmts for each
        // collected field initializer. Synthesize an empty ctor if
        // one wasn't declared so the inits still run on `new C(...)`.
        let ctor = self.finalize_class_field_inits(&name, field_inits, ctor);
        // P8.1 — restore the outer class context (parse-error paths
        // skip this; the parser is in an error state and the value
        // is moot).
        self.current_class = saved_class;
        self.current_class_has_parent = saved_has_parent;
        self.super_prop_allowed = saved_super_prop;
        self.pop_class_scope(&name, &fields, &static_init, &methods, &static_methods)?;
        Ok(Stmt::ClassDecl {
            name,
            type_params,
            parent,
            is_abstract,
            fields,
            static_init,
            ctor,
            methods,
            static_methods,
        })
    }
}
