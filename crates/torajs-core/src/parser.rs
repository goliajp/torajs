//! Recursive descent parser. Grammar:
//!
//! program  := stmt*
//! stmt     := decl | if | while | block | fndecl | return | expr `;`?
//! decl     := (`let` | `const`) IDENT (`:` IDENT)? `=` expr `;`?
//! if       := `if` `(` expr `)` stmt (`else` stmt)?
//! while    := `while` `(` expr `)` stmt
//! block    := `{` stmt* `}`
//! fndecl   := `function` IDENT `(` params? `)` (`:` IDENT)? `{` stmt* `}`
//! params   := param (`,` param)*
//! param    := IDENT (`:` IDENT)?
//! return   := `return` expr? `;`?
//! expr     := assign
//! assign   := lor (`=` assign)?                    (* right-associative *)
//! lor      := land (`||` land)*
//! land     := bit_or (`&&` bit_or)*
//! bit_or   := bit_xor (`|` bit_xor)*
//! bit_xor  := bit_and (`^` bit_and)*
//! bit_and  := equality (`&` equality)*
//! equality := comparison ((`===` | `!==`) comparison)*
//! comparison := shift ((`<`|`>`|`<=`|`>=`) shift)*
//! shift    := additive ((`<<`|`>>`) additive)*
//! additive := mul (( `+` | `-` ) mul)*
//! mul      := unary (( `*` | `/` | `%` ) unary)*
//! unary    := `!` unary | postfix
//! postfix  := primary ( `.` ident | `(` args `)` | `[` expr `]` )*
//! args     := (expr (`,` expr)*)?
//! primary  := ident | string | number | `true` | `false` | arrow_fn | array_lit
//! arrow_fn := `(` params? `)` (`:` IDENT)? `=>` (block | expr)
//! array_lit := `[` (expr (`,` expr)*)? `]`
//! type_ann := IDENT (`[` `]`)*

use crate::ast::{self, Ast, BinOp, ClassCtor, ClassMethod, Expr, ExprId, Param, StaticInit, Stmt};
use crate::lexer::{self, Spanned, Token};

mod class_member;
mod destr_drivers;
mod destr_helpers;
mod expr_entry;
mod expr_prec;
mod fn_expr;
mod object_literal;
mod object_member;
mod parse_class_decl;
mod parse_class_member_field;
mod parse_class_member_method;
mod parse_postfix;
mod parse_stmt;
mod try_parse_for_of;
mod type_ann;
mod type_decl;
use class_member::ClassMemberModifierPrefix;

pub fn parse(tokens: &[Spanned]) -> Result<Ast, String> {
    let mut ast = Ast::default();
    parse_into(tokens, &mut ast)?;
    Ok(ast)
}

/// Phase K.2 — append-mode parse. Parses `tokens` into the existing
/// `target` AST, sharing its `exprs` arena so any newly-minted ExprIds
/// continue numbering from `target.exprs.len()`. Returns the index of
/// the first appended Stmt in `target.stmts` (caller can drain from
/// there to extract just the new section).
///
/// Used by `modules::resolve_imports` to merge an imported file's AST
/// into the main file's AST without an ExprId remap pass — every Expr
/// landed via `add_expr`, which mints a fresh u32 from the current
/// `exprs.len()`, so values originating in the imported file are
/// already indexed correctly within the merged arena.
///
/// The Parser-internal `desugar_id` counter is seeded with the current
/// arena length so any temp-name minting (`__step_<n>`, etc.) emitted
/// by parse-time desugars in the imported file can't collide with
/// names already minted while parsing the main file (or any earlier
/// imported file).
pub fn parse_into(tokens: &[Spanned], target: &mut Ast) -> Result<usize, String> {
    let stmt_offset = target.stmts.len();
    let id_offset = target.exprs.len() as u32;
    let taken = std::mem::take(target);
    let mut p = Parser {
        tokens,
        pos: 0,
        type_close_peel: 0,
        ast: taken,
        desugar_id: id_offset,
        generator_fns: std::collections::HashMap::new(),
        current_class: None,
        synth_classes: Vec::new(),
        class_value_aliases: std::collections::HashMap::new(),
        dyn_import_counter: 0,
    };
    let result = p.parse_program();
    *target = p.ast;
    result?;
    Ok(stmt_offset)
}

struct Parser<'a> {
    tokens: &'a [Spanned],
    pos: usize,
    // S155 — `Foo<Bar<X>>` lexes the closing `>>` as one ShrShr token;
    // nested type-arg parsing peels it as two virtual `>`s. This counter
    // tracks how many `>`s of the current ShrShr/ShrShrShr at `pos` have
    // been peeled (0 = full token, 1 = one peel left for `>>`, 2 = both
    // peeled for `>>>`). `peek()` consults it so the second `>` shows
    // up as `Gt` for the outer caller. Reset by any non-Gt advance.
    type_close_peel: u8,
    ast: Ast,
    /// Monotone counter used by parse-time desugars (for-of, destructuring,
    /// template literal interpolation) to mint collision-free temp names.
    /// Starts at 0; each `mint_temp_id` returns + increments.
    desugar_id: u32,
    /// I.2 / J.3 — `function*` declarations seen so far, mapping name to
    /// the declared yield-value type annotation (`function* gen(): T`).
    /// for-of and `yield*` dispatch on this map: when the source is a
    /// direct call to a known generator factory, the desugar emits the
    /// iterator-protocol shape (next-loop) with the yield type
    /// propagated as `let v: T`. Anything else falls back to the array
    /// shape. Limitation: only direct call sites are detected — `let g
    /// = gen(); for (let v of g)` won't recognise `g` as an iterator.
    generator_fns: std::collections::HashMap<String, String>,
    /// P8.1 — name of the class whose body we are currently inside,
    /// or None outside any class. Set at the top of
    /// `parse_class_decl_with_abstract` (after the class name is
    /// captured), restored on its successful exit. Read by
    /// `member_name_after_dot` so `this.#x` (and any `#`-prefixed
    /// member access lexically inside the class body) can mangle to
    /// `__priv_<class>__<name>` at parse time. For non-`this`
    /// receivers (`c.#x` where c: C), parser-time mangling is not
    /// possible without static type info — those flow through as
    /// raw `#name` and typecheck/desugar can mangle later (P8.x).
    current_class: Option<String>,
    /// P8.5 — parser-synthesized ClassDecls produced when a class
    /// appears in expression position (`const F = class { ... }`,
    /// `new (class { ... })()`, etc.). Each entry is anonymous on
    /// the source side; parser mints `__ClassExpr_<id>` via
    /// `mint_desugar_id`. Drained at the end of every `parse_stmt`
    /// in `parse_program` and prepended to that stmt in `ast.stmts`,
    /// so declaration order (parent before child, synth before use)
    /// is preserved without re-splicing at AST root (which would
    /// break the `stmt_offset` contract of `parse_into`).
    synth_classes: Vec<Stmt>,
    /// P8.5 — narrow-surface alias map: const-bindings whose init is
    /// directly a class expression (`const F = class { ... }`) or
    /// directly another such binding (`const G = F`). Maps the const
    /// name to the underlying `__ClassExpr_<id>` synth name. Read by
    /// `parse_new` to rewrite `new F()` → `new __ClassExpr_<id>()`
    /// at parse time, dispatching through the static factory
    /// `__new___ClassExpr_<id>` synthesized by `desugar_classes`.
    /// Avoids a downstream dynamic-ctor-dispatch substrate (which
    /// would touch typecheck + ssa_lower) at the cost of these
    /// narrow-form limitations: (i) `let` and reassignment shapes
    /// fall through to the regular ident path (still error); (ii) no
    /// scope-tracking — an inner-fn-body `const F = class {}` would
    /// overwrite an outer alias of the same name (test262 corpus
    /// rarely collides; if it does, lift to a scope stack). The full
    /// dynamic-ctor-dispatch substrate is parked as an L3b follow-up.
    class_value_aliases: std::collections::HashMap<String, String>,
    /// P13-S5 — counter for synthetic namespace bindings minted by
    /// `parse_primary` when it encounters a dynamic `import("./y")`
    /// expression. Each occurrence prepends a synthetic
    /// `import * as __dyn_<n> from "./y"` to the stmt stream (via
    /// `synth_classes`) and rewrites the expression to
    /// `Promise.resolve(__dyn_<n>)`.
    dyn_import_counter: u32,
}

/// V3-18 wedge — strip the standard generator/iterator wrapper from
/// a return-type annotation per TS spec §3.6.4. Recognized shapes
/// (all single-arg, all collapsing to the inner yield type T):
///   Generator<T>          (also Generator<T, R, N> — extras ignored)
///   IterableIterator<T>
///   Iterator<T>
///   Iterable<T>
/// The parser's flat-ann encoder writes these as `Head<T>` (or
/// `Head<T|R|N>`), so a depth-aware scan for the first `<` and the
/// trailing `>` is enough.
/// V3-18 wedge — recognise a syntactic IdentifierName per JS spec
/// §11.6.2: ASCII letters / `_` / `$` for the first byte; same set
/// plus digits for the rest. Used to fold `obj["x"]` into
/// `obj.x` at parse time when the bracket-index is a string
/// literal whose content is a legal identifier; non-ident strings
/// (`obj["a-b"]`, `obj["1"]`, `obj[""]`) stay as Index so the
/// existing Array / String paths handle them.
fn is_identifier_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let first = bytes[0];
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return false;
    }
    for &b in &bytes[1..] {
        if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'$') {
            return false;
        }
    }
    true
}

fn unwrap_generator_return_ann(ann: &str) -> String {
    let Some(open) = ann.find('<') else {
        return ann.to_string();
    };
    if !ann.ends_with('>') {
        return ann.to_string();
    }
    let head = &ann[..open];
    if !matches!(
        head,
        "Generator" | "IterableIterator" | "Iterator" | "Iterable"
    ) {
        return ann.to_string();
    }
    let inner = &ann[open + 1..ann.len() - 1];
    // Take only the first type-arg (yield type). TS Generator has
    // additional Return/Next type args; the subset runtime collapses
    // them so dropping is the only sensible thing.
    let mut depth: i32 = 0;
    for (i, b) in inner.bytes().enumerate() {
        match b {
            b'<' | b'(' => depth += 1,
            b'>' | b')' => depth -= 1,
            b'|' if depth == 0 => return inner[..i].to_string(),
            _ => {}
        }
    }
    inner.to_string()
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        // S155 — within nested generic type args, `>>` / `>>>` peel into
        // multiple `>`s. When `type_close_peel` is non-zero the current
        // ShrShr/ShrShrShr token has had its leading `>`(s) consumed
        // virtually; report the next virtual `>` (or `>>` after one
        // peel from `>>>`).
        if self.type_close_peel > 0 {
            match &self.tokens[self.pos].token {
                Token::ShrShr if self.type_close_peel == 1 => return &Token::Gt,
                Token::ShrShrShr if self.type_close_peel == 1 => return &Token::ShrShr,
                Token::ShrShrShr if self.type_close_peel == 2 => return &Token::Gt,
                _ => {}
            }
        }
        &self.tokens[self.pos].token
    }

    fn at(&self) -> u32 {
        self.tokens[self.pos].span.start
    }

    /// v0.3 #4 DWARF — add an Expr to the arena AND record its source
    /// byte range. `start_pos` is the *token index* where the expr
    /// began (typically captured before recursive descent); end byte
    /// is taken from the token just consumed (`self.pos - 1`).
    /// Defaults to (0, 0) sentinel if either index is OOB so callers
    /// don't have to thread Option through.
    fn add_expr_at(&mut self, start_pos: usize, e: Expr) -> ExprId {
        let start = self
            .tokens
            .get(start_pos)
            .map(|t| t.span.start)
            .unwrap_or(0);
        let end = if self.pos > 0 {
            self.tokens
                .get(self.pos - 1)
                .map(|t| t.span.end)
                .unwrap_or(start)
        } else {
            start
        };
        let id = self.ast.add_expr(e);
        self.ast
            .set_expr_span(id, crate::lexer::Span { start, end });
        id
    }

    fn parse_program(&mut self) -> Result<(), String> {
        while !matches!(self.peek(), Token::Eof) {
            let stmt = self.parse_stmt()?;
            // P8.5 — flush parser-synthesized class expressions
            // (`__ClassExpr_<id>` ClassDecls produced by class-in-
            // expression-position) immediately before the stmt that
            // owns their use site. Preserves: (i) parent-before-child
            // for `class extends Parent` (Parent was pushed in an
            // earlier iteration); (ii) synth-before-use (Ident hop in
            // the about-to-be-pushed stmt). Append-only — does not
            // alter the `stmt_offset` contract that `parse_into` relies
            // on for module merging.
            if !self.synth_classes.is_empty() {
                let synth: Vec<Stmt> = std::mem::take(&mut self.synth_classes);
                self.ast.stmts.extend(synth);
            }
            self.ast.stmts.push(stmt);
        }
        Ok(())
    }

    fn parse_block(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `{`
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => return Err(format!("expected `}}`, got {t:?} at {}", self.at())),
        }
        Ok(Stmt::Block(stmts))
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `if`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `if`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let cond = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after if condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let then_branch = Box::new(self.parse_stmt()?);
        let else_branch = if matches!(self.peek(), Token::Else) {
            self.pos += 1;
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_branch,
            else_branch,
        })
    }

    fn parse_fn(&mut self, is_async: bool) -> Result<Stmt, String> {
        self.pos += 1; // consume `function`
        // Phase J — `function*` generator declaration. Optional `*` token
        // sandwiched between `function` and the name marks this fn as a
        // generator; post-parse `desugar_generators` rewrites the body
        // into a state-machine class.
        let is_generator = matches!(self.peek(), Token::Star);
        if is_generator {
            self.pos += 1;
        }
        let name = match self.peek() {
            Token::Ident(n) => n.clone(),
            t => {
                return Err(format!(
                    "expected function name, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        // M3 — optional type-parameter list: `function id<T, U>(...)`. If
        // present, the names are recorded on the FnDecl and the typechecker
        // treats them as placeholder types that don't resolve to a concrete
        // shape until each call site.
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
                                "expected type-parameter name, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    match self.peek() {
                        Token::Comma => self.pos += 1,
                        Token::Gt => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `>` in type parameters, got {t:?} at {}",
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
                        "expected `>` to close type parameters, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        // V3-18 wedge — function parameter destructuring pattern:
        //   function f([a, b]: number[])           — array form
        //   function f({ x, y }: { x:T, y:U })     — object form
        // Per ES spec §14.1.3 a BindingPattern (array or object) is a
        // valid FormalParameter. The lexer produces `[` / `{` where the
        // ident-name is expected; pre-fix the param parser bailed at
        // 'expected parameter name, got LBracket / LBrace'.
        //
        // Implementation: a destr pattern at the param position
        // synthesizes a fresh hidden binding name (`__param_destr_<id>`)
        // and accumulates per-element / per-field `let bound = synth[i]`
        // (or `synth.field`) into `param_destr_lets`, which is prepended
        // to the parsed body just before emitting Stmt::FnDecl.
        // Reserved-word fields go through keyword_property_name to match
        // the obj-literal / for-of-destr wedges already in tree.
        let mut param_destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                let is_rest = matches!(self.peek(), Token::DotDotDot);
                if is_rest {
                    self.pos += 1;
                }
                if !is_rest && matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    let synth = self.parse_destr_param(&mut param_destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    // P-PARSE.6 — whole-pattern default on a destr fn
                    // param: `function f({a, b} = {a:1, b:2}) {...}`.
                    // Mirror of the arrow-fn destr default added in
                    // the same wedge.
                    let default = if matches!(self.peek(), Token::Eq) {
                        self.pos += 1;
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    params.push(Param {
                        name: synth,
                        type_ann,
                        default,
                        is_rest: false,
                    });
                    match self.peek() {
                        Token::Comma => {
                            self.pos += 1;
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            continue;
                        }
                        Token::RParen => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `)` after destr param, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
                let pname = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected parameter name, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                // V3-18 wedge — optional parameter `name?: T`. TS spec
                // §3.9.2.4: `?` permits the call site to omit the arg.
                // Subset models it as Nullable<T>. When the caller omits
                // the arg, the param receives the implicit `null` default
                // (synthesized below) — this lets `f()` Just Work for
                // `function f(x?: T)`, matching TS semantics where the
                // omitted `x` is `undefined` (subset's null sentinel).
                let optional = !is_rest && matches!(self.peek(), Token::Question);
                if optional {
                    self.pos += 1;
                }
                let type_ann = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    let ann = self.parse_type_ann()?;
                    if optional && !ann.starts_with("__nullable(") {
                        Some(format!("__nullable({ann})"))
                    } else {
                        Some(ann)
                    }
                } else {
                    None
                };
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else if optional {
                    // Implicit null default for `name?: T` without an
                    // explicit `= <expr>`. apply_default_args picks this
                    // up at every call site that omits the trailing arg.
                    Some(self.ast.add_expr(Expr::Null))
                } else {
                    None
                };
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest,
                });
                match self.peek() {
                    Token::Comma => {
                        if is_rest {
                            return Err(format!("rest parameter must be last at {}", self.at()));
                        }
                        self.pos += 1;
                        // V3-18 wedge — trailing comma in fn-decl param list.
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
                    Token::RParen => break,
                    t => return Err(format!("expected `,` or `)`, got {t:?} at {}", self.at())),
                }
            }
        }
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
        }
        let mut return_type = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        // I.2 / J.3 — record this generator so for-of and yield* can
        // look up its yield type at desugar time. The yield-type ann is
        // mandatory at desugar_generators (panics otherwise) so we
        // require it here too — without it the for-of body's `let v`
        // can't be typed.
        if is_generator {
            // P10.7 — Default-Any generator. When the user omits the
            // return-type annotation (`function* foo() {...}`), record
            // the yield type as `"any"` so for-of's body let-binding and
            // yield* delegation's intermediate vars flow through the
            // Any-tier. `ast::desugar_generators` mirrors the same
            // fallback (panic-on-None there now defaults to `"any"`).
            let raw_ann = return_type.clone().unwrap_or_else(|| "any".into());
            // V3-18 wedge — unwrap the standard wrapper-form return
            // type annotations users write for generators per TS spec
            // §3.6.4 (IterableIterator / Generator / Iterator). The
            // yield type T inside one of these wrappers is what the
            // Phase J desugar machinery needs — it builds the iterator
            // class layout from T, not from `Generator<T>`. Pre-fix
            // those wrapped anns failed at `unknown type` in check.rs.
            // Rewrite the FnDecl's return_type too so desugar_generators
            // sees the unwrapped T directly.
            let yield_ty = unwrap_generator_return_ann(&raw_ann);
            if yield_ty != raw_ann {
                return_type = Some(yield_ty.clone());
            }
            self.generator_fns.insert(name.clone(), yield_ty);
        }
        if is_async {
            self.ast.async_fns.insert(name.clone());
        }
        // V3-18 wedge — TS overload signature: `function f(...): R;`
        // (no body, terminated by `;`). Type-only; the runtime impl
        // is the trailing same-named declaration. Discard by
        // returning an empty Block — the real FnDecl follows.
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            return Ok(Stmt::Block(Vec::new()));
        }
        // body must be a block
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` (function body), got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            body.push(self.parse_stmt()?);
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => return Err(format!("expected `}}`, got {t:?} at {}", self.at())),
        }
        // V3-18 wedge — prepend per-param destructuring lets when a
        // parameter was a binding pattern. Order is preserved (lets
        // run first, then user body).
        if !param_destr_lets.is_empty() {
            let mut full = param_destr_lets;
            full.extend(body);
            body = full;
        }
        Ok(Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type,
            body,
            is_generator,
        })
    }

    fn parse_return(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `return`
        let expr = match self.peek() {
            Token::Semi | Token::RBrace | Token::Eof => None,
            _ => Some(self.parse_expr()?),
        };
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Return(expr))
    }

    fn parse_throw(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `throw`
        let expr = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::Throw(expr))
    }

    /// `try { body } catch (e) { catch_body } [finally { finally_body }]`.
    /// `catch (e)` is required for now — TS allows `try { } finally { }`
    /// without catch but our M4.1 surface requires the catch.
    fn parse_try(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `try`
        let body = self.parse_block_stmts("try")?;
        // TS allows `try { } catch { }` OR `try { } finally { }` OR
        // `try { } catch { } finally { }` — at least one of catch /
        // finally is required.
        let mut catch_param: Option<String> = None;
        let mut catch_type: Option<String> = None;
        let mut catch_body: Vec<Stmt> = Vec::new();
        let mut had_catch = false;
        // P4.7 — captured (binding-name, source-key, is_obj) tuples
        // when the catch param is a destructure pattern. After parsing
        // the catch body, we synthesize lets at the top of the body
        // that read each binding from the synthetic catch ident.
        let mut destr_bindings: Vec<(String, String, bool)> = Vec::new();
        if matches!(self.peek(), Token::Catch) {
            had_catch = true;
            self.pos += 1;
            // `catch (e[: T])` is optional since ES2019.
            if matches!(self.peek(), Token::LParen) {
                self.pos += 1;
                let n = match self.peek() {
                    Token::Ident(n) => {
                        let s = n.clone();
                        self.pos += 1;
                        s
                    }
                    // P4.7 — destructuring catch parameter
                    // (`catch ({ x, y }) {}`). ES2018+ BindingPattern
                    // syntax. tora parses the pattern at depth 1,
                    // captures the binding names, and (after the
                    // catch body parses) prepends synthetic
                    // `const x: any = __catch.x;` lets so the body's
                    // refs resolve. The synth catch param is typed
                    // `any` so the Member accesses route through
                    // dynobj_get via the Any-tier. Array destructure
                    // `catch ([a, b])` follows the same shape with
                    // string-keyed numeric indices.
                    Token::LBrace | Token::LBracket => {
                        let is_obj = matches!(self.peek(), Token::LBrace);
                        let synth_name = format!("__catch_destr_{}", self.pos);
                        self.pos += 1; // skip opener
                        let mut idx = 0u32;
                        while !matches!(self.peek(), Token::RBrace | Token::RBracket | Token::Eof) {
                            let bind_name = match self.peek() {
                                Token::Ident(n) => {
                                    let s = n.clone();
                                    self.pos += 1;
                                    s
                                }
                                t => {
                                    return Err(format!(
                                        "expected identifier in catch destructure, got {t:?} at {}",
                                        self.at()
                                    ));
                                }
                            };
                            let stored_name = if is_obj && matches!(self.peek(), Token::Colon) {
                                self.pos += 1;
                                match self.peek() {
                                    Token::Ident(n) => {
                                        let s = n.clone();
                                        self.pos += 1;
                                        s
                                    }
                                    t => {
                                        return Err(format!(
                                            "expected alias after `:` in catch destructure, got {t:?} at {}",
                                            self.at()
                                        ));
                                    }
                                }
                            } else {
                                bind_name.clone()
                            };
                            let src_key = if is_obj {
                                bind_name
                            } else {
                                format!("{}", idx)
                            };
                            destr_bindings.push((stored_name, src_key, is_obj));
                            idx += 1;
                            if matches!(self.peek(), Token::Comma) {
                                self.pos += 1;
                            }
                        }
                        match self.peek() {
                            Token::RBrace | Token::RBracket => self.pos += 1,
                            _ => {}
                        }
                        synth_name
                    }
                    t => {
                        return Err(format!(
                            "expected catch parameter name, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                let ty = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    Some(self.parse_type_ann()?)
                } else {
                    None
                };
                match self.peek() {
                    Token::RParen => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `)` after catch param, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                catch_param = Some(n.clone());
                catch_type = ty;
                // P4.7 — destructure forces `: any` catch type so the
                // synthesized Member-reads route through the Any-tier.
                if !destr_bindings.is_empty() {
                    catch_type = Some("any".to_string());
                }
            }
            catch_body = self.parse_block_stmts("catch")?;
            // P4.7 — synthesize `const <bind>: any = <__catch>.<src_key>;`
            // for each destructure binding and prepend to catch_body.
            if !destr_bindings.is_empty()
                && let Some(catch_n) = catch_param.as_ref()
            {
                let mut prepend: Vec<Stmt> = Vec::with_capacity(destr_bindings.len());
                for (bind_name, src_key, _is_obj) in destr_bindings.iter() {
                    let obj_eid = self.ast.add_expr(Expr::Ident(catch_n.clone()));
                    let member_eid = self.ast.add_expr(Expr::Member {
                        obj: obj_eid,
                        name: src_key.clone(),
                    });
                    prepend.push(Stmt::LetDecl {
                        mutable: false,
                        name: bind_name.clone(),
                        type_ann: Some("any".to_string()),
                        init: member_eid,
                        is_var: false,
                    });
                }
                let mut new_body: Vec<Stmt> = Vec::with_capacity(catch_body.len() + prepend.len());
                new_body.extend(prepend);
                new_body.extend(catch_body.drain(..));
                catch_body = new_body;
            }
        }
        let finally_body = if matches!(self.peek(), Token::Finally) {
            self.pos += 1;
            Some(self.parse_block_stmts("finally")?)
        } else {
            None
        };
        if !had_catch && finally_body.is_none() {
            return Err(format!(
                "try block needs `catch` or `finally` (or both); got {:?} at {}",
                self.peek(),
                self.at()
            ));
        }
        Ok(Stmt::Try {
            body,
            had_catch,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        })
    }

    /// Parse a `{ ... }` block as a flat list of statements (used by try /
    /// catch / finally where we want the inner stmts directly, not wrapped
    /// in `Stmt::Block`).
    fn parse_block_stmts(&mut self, ctx: &str) -> Result<Vec<Stmt>, String> {
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin {ctx} block, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            stmts.push(self.parse_stmt()?);
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end {ctx} block, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(stmts)
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `while`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `while`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let cond = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after while condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::While { cond, body })
    }

    fn parse_do_while(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `do`
        let body = Box::new(self.parse_stmt()?);
        match self.peek() {
            Token::While => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `while` after `do {{ … }}`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `while` in do-while, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let cond = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after do-while condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // Optional `;` after the closing paren.
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
        Ok(Stmt::DoWhile { body, cond })
    }

    fn parse_switch(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `switch`
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `switch`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let scrutinee = self.parse_expr()?;
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after switch scrutinee, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        match self.peek() {
            Token::LBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `{{` to begin switch body, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let mut cases: Vec<ast::SwitchCase> = Vec::new();
        let mut default: Option<Vec<Stmt>> = None;
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            match self.peek() {
                Token::Case => {
                    self.pos += 1;
                    let value = self.parse_expr()?;
                    match self.peek() {
                        Token::Colon => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `:` after case value, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    let mut body: Vec<Stmt> = Vec::new();
                    while !matches!(
                        self.peek(),
                        Token::Case | Token::Default | Token::RBrace | Token::Eof
                    ) {
                        body.push(self.parse_stmt()?);
                    }
                    cases.push(ast::SwitchCase { value, body });
                }
                Token::Default => {
                    self.pos += 1;
                    match self.peek() {
                        Token::Colon => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `:` after `default`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    let mut body: Vec<Stmt> = Vec::new();
                    while !matches!(
                        self.peek(),
                        Token::Case | Token::Default | Token::RBrace | Token::Eof
                    ) {
                        body.push(self.parse_stmt()?);
                    }
                    default = Some(body);
                }
                t => {
                    return Err(format!(
                        "expected `case` or `default` inside switch, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
        }
        match self.peek() {
            Token::RBrace => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `}}` to end switch, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(Stmt::Switch {
            scrutinee,
            cases,
            default,
        })
    }

    /// `for (init?; cond?; step?) body`. Each clause is optional but the
    /// two `;` separators are required (matches TS / C). Init is parsed
    /// as a stmt (typically a `let` decl or expr-stmt). Cond is an expr.
    /// Step is an expr (we don't have post-increment yet — use
    /// `i = i + 1`).
    fn mint_desugar_id(&mut self) -> u32 {
        let id = self.desugar_id;
        self.desugar_id += 1;
        id
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `for`
        // P10.3-A1 — `for await (decl of iter)`. After consuming the
        // optional `await` keyword the rest of the head parses
        // identically to a plain `for (...)` for-of. The desugar
        // wraps the per-iteration element-load in a Member `.value`
        // access (await desugar) so the user's binding sees the
        // resolved T instead of Promise<T>. Only the for-of arm of
        // try_parse_for_of honors is_async; if the head turns out
        // to be a C-style for, we emit a clean error since `for
        // await` requires the iterable form.
        let is_async = if matches!(self.peek(), Token::Await) {
            self.pos += 1;
            true
        } else {
            false
        };
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `(` after `for`, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // for-of detection: `for ( (let|const) IDENT (`:` T)? "of" EXPR )`.
        // `of` is a contextual keyword (Token::Ident("of")) so this only
        // triggers when the user means it. Falls back to C-style for-loop
        // otherwise. The desugar produces zero-overhead SSA: source is
        // bound once into a `__forof_src_N` temp (skipped if the source
        // was already an Ident — mem2reg would elide it anyway, but we
        // skip eagerly so the AST stays small), `__forof_end_N` caches
        // length, classic for-loop walks indices, body sees the user's
        // binding rebound from `__src[__i]`.
        if let Some(stmt) = self.try_parse_for_of(is_async)? {
            return Ok(stmt);
        }
        if is_async {
            return Err(format!(
                "`for await` requires the iterable form `for await (const x of iter)` at {}",
                self.at()
            ));
        }
        // init clause — empty (just `;`) or any stmt that ends with `;`.
        let init = if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
            None
        } else {
            // parse_stmt eats its own trailing `;` for let / expr stmts.
            Some(Box::new(self.parse_stmt()?))
        };
        // cond clause — empty means infinite-loop (true). Empty is `;`.
        let cond = if matches!(self.peek(), Token::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        match self.peek() {
            Token::Semi => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `;` after `for` condition, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        // step clause — empty means no step.
        // V3-18 m1.h.31 — JS allows comma-separated step expressions
        // (`for (...; ...; i++, j--)`). Parse them as a chained
        // Expr::Sequence so the lowerer evaluates each in order.
        let step = if matches!(self.peek(), Token::RParen) {
            None
        } else {
            let mut s = self.parse_expr()?;
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                let next = self.parse_expr()?;
                s = self.ast.add_expr(Expr::Sequence {
                    left: s,
                    right: next,
                });
            }
            Some(s)
        };
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after `for` step, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::For {
            init,
            cond,
            step,
            body,
        })
    }

    /// V3-18 wedge — return the source spelling of `t` if it's a
    /// reserved-word keyword that can appear in property-name
    /// contexts (object-literal field, member access, destructuring
    /// pattern, class member). Per ES spec §12.7.6 IdentifierName
    /// allows the full reserved-word list at these positions; TS
    /// follows. Used by member_name_after_dot, parse_object_field,
    /// parse_object_destructuring, and the class-member-name branch
    /// in parse_class — all four had their own short keyword whitelist
    /// that drifted apart over time. Centralized here.
    fn keyword_property_name(t: &Token) -> Option<&'static str> {
        Some(match t {
            Token::Catch => "catch",
            Token::Finally => "finally",
            Token::Return => "return",
            Token::Throw => "throw",
            Token::If => "if",
            Token::Else => "else",
            Token::For => "for",
            Token::While => "while",
            Token::Do => "do",
            Token::Break => "break",
            Token::Continue => "continue",
            Token::Switch => "switch",
            Token::Case => "case",
            Token::Default => "default",
            Token::Class => "class",
            Token::New => "new",
            Token::This => "this",
            Token::Function => "function",
            Token::TypeOf => "typeof",
            Token::InstanceOf => "instanceof",
            Token::Try => "try",
            Token::Yield => "yield",
            // Extended set — these were rejected pre-wedge in
            // every property-name position.
            Token::Type => "type",
            Token::Async => "async",
            Token::Await => "await",
            Token::Import => "import",
            Token::Export => "export",
            Token::Null => "null",
            Token::True => "true",
            Token::False => "false",
            Token::Let => "let",
            Token::Const => "const",
            Token::Extends => "extends",
            Token::Super => "super",
            Token::Void => "void",
            _ => return None,
        })
    }

    /// Identifier-or-contextual-keyword name after a `.` / `?.` / for
    /// class member declaration. JS / TS allow reserved words to appear
    /// as property names (`p.catch(...)`, `obj.return`) — routes
    /// keyword tokens through `keyword_property_name` for the full
    /// reserved-word list. Advances `self.pos` on success and returns
    /// None (without consuming) when no name token is present.
    fn member_name_after_dot(&mut self) -> Option<String> {
        if let Token::Ident(n) = self.peek() {
            let n = n.clone();
            self.pos += 1;
            return Some(n);
        }
        // P8.1 — `#name` PrivateIdentifier after dot. When we are
        // lexically inside a class body (`current_class` is Some) the
        // access is necessarily through `this.#x` or `c.#x` for a `c`
        // typed as the same class — both bind to the SAME class's
        // private slot, so mangling to `__priv_<current_class>__<n>`
        // at parse time is correct and matches the field-decl
        // mangling that A2 wrote into struct_layouts. Cross-class
        // access (e.g. inside D's method, `c: C` and `c.#x`) is
        // rejected by the existing `Visibility::Private` exact-class
        // check in check.rs (M-OO.5 — exact-class match for
        // Private), because the mangled name carries the declaring
        // class in its prefix.
        //
        // Outside any class (`current_class` is None) `#name` access
        // has no defined binding — keep the raw `#`-prefixed name so
        // the type / lookup layer fails cleanly with "no field `#x`
        // on Struct" instead of accidentally finding a Public field.
        if let Token::PrivateIdent(n) = self.peek() {
            let n = n.clone();
            self.pos += 1;
            return Some(match &self.current_class {
                Some(cls) => format!("__priv_{cls}__{n}"),
                None => format!("#{n}"),
            });
        }
        let kw = Self::keyword_property_name(self.peek())?;
        self.pos += 1;
        Some(kw.to_string())
    }

    fn parse_primary(&mut self) -> Result<ExprId, String> {
        // P13-S5 — dynamic `import("./y")` expression. Restricted to a
        // string-literal source (compile-time resolvable for AOT) so we
        // can synthesize a `import * as __dyn_<n> from "./y"` decl in
        // the same translation unit and rewrite the expression to
        // `Promise.resolve(__dyn_<n>)`. The runtime path is identical
        // to the static namespace-import form (P13-S2) — the only
        // observable difference is the `Promise<…>` wrapper, matching
        // `import()` per ES §13.3.10.
        if matches!(self.peek(), Token::Import) {
            self.pos += 1;
            if !matches!(self.peek(), Token::LParen) {
                return Err(format!(
                    "dynamic `import` requires `(<source>)`; got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
            self.pos += 1;
            let source = match self.peek() {
                Token::String(s) => {
                    let s = s.clone();
                    self.pos += 1;
                    s
                }
                t => {
                    return Err(format!(
                        "dynamic `import` requires a string-literal source; got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            if !matches!(self.peek(), Token::RParen) {
                return Err(format!(
                    "expected `)` to close dynamic `import(...)`, got {:?} at {}",
                    self.peek(),
                    self.at()
                ));
            }
            self.pos += 1;
            let n = self.dyn_import_counter;
            self.dyn_import_counter += 1;
            let ns_name = format!("__dyn_ns_{n}");
            // Synthesize: `import * as <ns_name> from <source>`. The
            // existing K.2 namespace pipeline (P13-S2) materializes the
            // alias as a struct literal of all of `source`'s exports.
            self.synth_classes.push(Stmt::ImportDecl {
                default: None,
                namespace: Some(ns_name.clone()),
                named: Vec::new(),
                source,
            });
            // Build expression: `{ value: <ns_name> }`. The await prefix
            // desugar (`<expr>.value`) then reads the namespace struct
            // out of the wrapper. This matches the common
            // `const M = await import("./y")` pattern without forcing the
            // typecheck through Promise<struct-with-fn-fields> (which is
            // a separate substrate gap tracked in L3b — the standalone
            // Promise<namespace> shape only matters for `.then()` use,
            // which is uncommon for dynamic import in TS code).
            let ns_id = self.ast.add_expr(Expr::Ident(ns_name));
            let wrapper = self.ast.add_expr(Expr::ObjectLit {
                fields: vec![("value".into(), ns_id)],
            });
            return Ok(wrapper);
        }
        if matches!(self.peek(), Token::LParen) {
            // `(` could start either a parenthesized expression `(e)` or an
            // arrow fn `(params) =>`. Disambiguate by scanning forward to the
            // matching `)` and peeking for `=>`.
            if self.is_arrow_fn_at_lparen() {
                return self.parse_arrow_fn();
            }
            self.pos += 1; // consume `(`
            // V3-18 m1.h.6 — JS spec §13.16 comma operator inside
            // parentheses: `(a, b, c)` evaluates left-to-right and
            // returns the rightmost value. Earlier subexpressions
            // are still type-checked for side effects but their
            // values are discarded. Encoded as nested
            // Expr::Sequence (using a temp let + discard pattern at
            // lower time) — for the MVP we simply discard left
            // values and return the rightmost expression's id.
            let mut last = self.parse_expr()?;
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                let next = self.parse_expr()?;
                last = self.ast.add_expr(Expr::Sequence {
                    left: last,
                    right: next,
                });
            }
            match self.peek() {
                Token::RParen => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `)` after parenthesized expression, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            return Ok(last);
        }
        if matches!(self.peek(), Token::LBracket) {
            return self.parse_array_literal();
        }
        if matches!(self.peek(), Token::LBrace) {
            // `{` in expression position is an object literal. Block
            // statements are caught by `parse_stmt`'s LBrace check before
            // reaching here, so the only path that lands at LBrace in
            // primary is an expression context (let-init, fn arg, return
            // value, etc.).
            return self.parse_object_literal();
        }
        // Function expression — `function (params): R { body }` or
        // `function NAME(params): R { body }` in expression position.
        // IIFE pattern `(function() { ... }())` is the dominant test262
        // shape this unblocks. Treat it as an `Expr::ArrowFn`: lifted by
        // `lift_arrow_fns` to a top-level FnDecl, same downstream
        // pipeline as `() => { ... }`. The optional name is parsed
        // (and ignored — fn-expr names are scoped only to the body, a
        // niche we don't implement).
        if matches!(self.peek(), Token::Function) {
            return self.parse_fn_expr();
        }
        // P8.5 — class expression in expression position
        // (ES spec §15.7.4 ClassExpression). Forms:
        //   const F = class { ... }                  // anonymous
        //   const F = class Inner { ... }            // named (inner
        //                                            //   self-binding
        //                                            //   not yet wired)
        //   const F = class extends Parent { ... }   // extends
        //   foo(class { ... })                       // class as argument
        // Strategy (a): parse as a ClassDecl with an `__ClassExpr_<id>`
        // synth name, buffer to `synth_classes` for splice immediately
        // before this stmt's push in `parse_program`, then emit
        // `Expr::Ident("__ClassExpr_<id>")` at the original use site.
        // The existing class machinery (desugar_classes,
        // synthesize_class_globals) lifts this Ident to the value-form
        // class global with zero changes downstream. Spec deviations
        // documented as L3b: anonymous `.name === "__ClassExpr_<id>"`
        // (should be `""`); named `Inner` self-binding inside body
        // doesn't resolve to the synth name.
        if matches!(self.peek(), Token::Class) {
            let stmt = self.parse_class_decl_with_abstract(false, true, true)?;
            let cls_name = match &stmt {
                Stmt::ClassDecl { name, .. } => name.clone(),
                _ => unreachable!("parse_class_decl_with_abstract returns ClassDecl"),
            };
            self.synth_classes.push(stmt);
            return Ok(self.ast.add_expr(Expr::Ident(cls_name)));
        }
        // P0.10 — async function expression `async function() {...}` /
        // `async function NAME() {...}` per ES spec §15.8.5
        // AsyncFunctionExpression. Used in test262 for `async function()
        // {}.constructor` (~15+ cases under built-ins/AsyncFunction/* and
        // built-ins/AsyncDisposableStack/*). Real async-fn-expression
        // substrate (state-machine generation, await binding) is a
        // P-LATER item; for the parser milestone we accept the syntax,
        // brace-balance the body, and emit an empty placeholder
        // Expr::ArrowFn — same strategy as generator-expression
        // (function*) and getter/setter/computed-method bodies.
        // P1 — async arrow functions `async x => ...` / `async (a, b)
        // => ...`. tora's regular arrow parser doesn't recognize the
        // `async` prefix in expression position (the keyword is
        // distinct from Ident "async"). Stub the syntax by dropping
        // the body — same opaque-stub strategy as async function
        // expression (real await binding / state-machine substrate
        // is P-LATER). Detected forms:
        //   async Ident => <expr | { body }>
        //   async (Params) => <expr | { body }>
        if matches!(self.peek(), Token::Async)
            && let Some(t1) = self.tokens.get(self.pos + 1)
            && (matches!(t1.token, Token::Ident(_)) || matches!(t1.token, Token::LParen))
        {
            // Distinguish `async function ...` (handled below) from
            // an async arrow. The Function check is later in the if
            // chain — here both Ident and LParen lead to arrow form.
            // For Ident case, peek+2 must be FatArrow.
            let is_arrow = if matches!(t1.token, Token::Ident(_)) {
                self.tokens
                    .get(self.pos + 2)
                    .is_some_and(|t| matches!(t.token, Token::FatArrow))
            } else {
                // LParen — scan to matching RParen, then check for FatArrow
                let mut j = self.pos + 2;
                let mut depth = 1i32;
                while depth > 0 && j < self.tokens.len() {
                    match self.tokens[j].token {
                        Token::LParen => depth += 1,
                        Token::RParen => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                self.tokens
                    .get(j)
                    .is_some_and(|t| matches!(t.token, Token::FatArrow))
            };
            if is_arrow {
                self.pos += 1; // consume `async`
                // Drop the param list / single Ident.
                if matches!(self.peek(), Token::Ident(_)) {
                    self.pos += 1; // single-param shorthand
                } else {
                    // (...)
                    self.pos += 1; // consume LParen
                    let mut depth = 1i32;
                    while depth > 0 {
                        match self.peek() {
                            Token::LParen => depth += 1,
                            Token::RParen => depth -= 1,
                            Token::Eof => {
                                return Err(format!(
                                    "unexpected eof in async arrow params at {}",
                                    self.at()
                                ));
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                }
                // Consume FatArrow.
                self.pos += 1;
                // Body — either expression or block.
                if matches!(self.peek(), Token::LBrace) {
                    self.pos += 1;
                    let mut depth = 1i32;
                    while depth > 0 {
                        match self.peek() {
                            Token::LBrace => depth += 1,
                            Token::RBrace => depth -= 1,
                            Token::Eof => {
                                return Err(format!(
                                    "unexpected eof in async arrow body at {}",
                                    self.at()
                                ));
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                } else {
                    // Expression body — parse and discard.
                    let _ = self.parse_assign()?;
                }
                return Ok(self.ast.add_expr(Expr::ArrowFn {
                    params: Vec::new(),
                    return_type: None,
                    body: Vec::new(),
                }));
            }
        }
        if matches!(self.peek(), Token::Async)
            && let Some(next) = self.tokens.get(self.pos + 1)
            && matches!(next.token, Token::Function)
        {
            self.pos += 2; // consume `async function`
            // P1 — `async function*` (async generator) is also accepted
            // and stubbed via the same drop-the-body strategy. Consume
            // the optional `*` token.
            if matches!(self.peek(), Token::Star) {
                self.pos += 1;
            }
            // Optional name — accept and discard.
            if let Token::Ident(_) = self.peek() {
                self.pos += 1;
            }
            // Drop the param list paren-balanced.
            match self.peek() {
                Token::LParen => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `(` after async function expression, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let mut depth: i32 = 1;
            while depth > 0 {
                match self.peek() {
                    Token::LParen => depth += 1,
                    Token::RParen => depth -= 1,
                    Token::Eof => {
                        return Err(format!(
                            "unexpected EOF inside async function expression param list at {}",
                            self.at()
                        ));
                    }
                    _ => {}
                }
                self.pos += 1;
            }
            // Optional return-type annotation.
            if matches!(self.peek(), Token::Colon) {
                self.pos += 1;
                let _ = self.parse_type_ann()?;
            }
            // Drop the body brace-balanced.
            match self.peek() {
                Token::LBrace => self.pos += 1,
                t => {
                    return Err(format!(
                        "expected `{{` after async function expression header, got {t:?} at {}",
                        self.at()
                    ));
                }
            }
            let mut depth: i32 = 1;
            while depth > 0 {
                match self.peek() {
                    Token::LBrace => depth += 1,
                    Token::RBrace => depth -= 1,
                    Token::Eof => {
                        return Err(format!(
                            "unexpected EOF inside async function expression body at {}",
                            self.at()
                        ));
                    }
                    _ => {}
                }
                self.pos += 1;
            }
            return Ok(self.ast.add_expr(Expr::ArrowFn {
                params: Vec::new(),
                return_type: None,
                body: Vec::new(),
            }));
        }
        // Regex literal `/pattern/flags`. The lexer already
        // disambiguated regex vs division by inspecting the previous
        // token; the parser just unwraps the carried pattern + flags
        // into the AST node. check.rs rejects the resulting Expr::Regex
        // with a "regex literals not yet implemented" message — the
        // matching engine is a follow-up phase. Parsing accept here
        // unblocks the lex / parse error buckets ahead of that work.
        if let Token::Regex { pattern, flags } = self.peek().clone() {
            self.pos += 1;
            return Ok(self.ast.add_expr(Expr::Regex { pattern, flags }));
        }
        let pos = self.pos;
        /* Single-param arrow without parens: `x => body`. JS spec
         * accepts this as shorthand for `(x) => body` (the `x` has
         * no type annotation; tr's check.rs auto-promotes untyped
         * params to fresh type params via desugar_implicit_generics).
         * Detect by peeking Ident + FatArrow before falling through
         * to the regular Ident expression. */
        if let Token::Ident(n) = &self.tokens[pos].token {
            let next_is_arrow = self
                .tokens
                .get(pos + 1)
                .map(|t| matches!(t.token, Token::FatArrow))
                .unwrap_or(false);
            if next_is_arrow {
                let pname = n.clone();
                self.pos += 2; /* consume Ident + FatArrow */
                let body = if matches!(self.peek(), Token::LBrace) {
                    self.pos += 1;
                    let mut stmts = Vec::new();
                    while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                        stmts.push(self.parse_stmt()?);
                    }
                    match self.peek() {
                        Token::RBrace => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `}}` after arrow body, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    stmts
                } else {
                    let e = self.parse_expr()?;
                    vec![Stmt::Return(Some(e))]
                };
                return Ok(self.ast.add_expr(Expr::ArrowFn {
                    params: vec![Param {
                        name: pname,
                        type_ann: None,
                        default: None,
                        is_rest: false,
                    }],
                    return_type: None,
                    body,
                }));
            }
        }
        match &self.tokens[pos].token {
            Token::Ident(n) => {
                let n = n.clone();
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Ident(n)))
            }
            Token::String(s) => {
                let s = s.clone();
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::String(s)))
            }
            Token::Template { parts } => {
                let parts = parts.clone();
                self.pos += 1;
                self.lower_template_parts(&parts)
            }
            Token::Number(n) => {
                let n = *n;
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Number(n)))
            }
            Token::BigInt { digits, radix } => {
                let digits = digits.clone();
                let radix = *radix;
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::BigInt { digits, radix }))
            }
            Token::True => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Bool(true)))
            }
            Token::False => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Bool(false)))
            }
            Token::Null => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::Null))
            }
            Token::This => {
                self.pos += 1;
                Ok(self.ast.add_expr(Expr::This))
            }
            Token::Super => {
                // `super(args)` — only valid inside a subclass ctor; the
                // desugar pass enforces that and rewrites to a Call to
                // `__cm_<Parent>__ctor(__this, args)`.
                // V3-18 wedge — `super.<method>(args)` (explicit
                // parent-method call): encoded as a Call to a marker
                // ident `__supercall__<methodname>`; desugar_classes
                // rewrites it to `__cm_<Parent>__<m>(__this, args)`
                // using the surrounding class's parent.
                self.pos += 1;
                if matches!(self.peek(), Token::Dot) {
                    self.pos += 1;
                    let m_name = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected method name after `super.`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    match self.peek() {
                        Token::LParen => self.pos += 1,
                        t => {
                            return Err(format!(
                                "expected `(` after `super.{m_name}`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                    let mut args: Vec<ExprId> = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        args.push(self.parse_expr()?);
                        while matches!(self.peek(), Token::Comma) {
                            self.pos += 1;
                            args.push(self.parse_expr()?);
                        }
                    }
                    match self.peek() {
                        Token::RParen => self.pos += 1,
                        t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
                    }
                    let callee = self
                        .ast
                        .add_expr(Expr::Ident(format!("__supercall__{m_name}")));
                    return Ok(self.ast.add_expr(Expr::Call { callee, args }));
                }
                match self.peek() {
                    Token::LParen => self.pos += 1,
                    t => {
                        return Err(format!(
                            "expected `(` after `super`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
                let mut args: Vec<ExprId> = Vec::new();
                if !matches!(self.peek(), Token::RParen) {
                    args.push(self.parse_expr()?);
                    while matches!(self.peek(), Token::Comma) {
                        self.pos += 1;
                        args.push(self.parse_expr()?);
                    }
                }
                match self.peek() {
                    Token::RParen => self.pos += 1,
                    t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
                }
                Ok(self.ast.add_expr(Expr::Super { args }))
            }
            Token::New => {
                // `new ClassName(args)` — type args / generic ctors not yet
                // supported; that's M5.2 alongside extends.
                self.pos += 1;
                // P4.5 — `new.target` meta-property. Spec §13.3.10:
                // evaluates to the [[NewTarget]] of the current
                // execution context. Recognized at the parser layer
                // because `new` followed by `.` would otherwise hit the
                // class-name error path.
                if matches!(self.peek(), Token::Dot) {
                    self.pos += 1;
                    match self.peek() {
                        Token::Ident(n) if n == "target" => {
                            self.pos += 1;
                            return Ok(self.ast.add_expr(Expr::NewTarget));
                        }
                        t => {
                            return Err(format!(
                                "expected `target` after `new.`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
                let class_name = match self.peek() {
                    Token::Ident(n) => {
                        let n = n.clone();
                        self.pos += 1;
                        // P8.5 — narrow alias resolution. If `new F()`
                        // and F is a const-bound class expression,
                        // rewrite to `new __ClassExpr_<id>()` so the
                        // static-factory path resolves to the synth
                        // class's `__new___ClassExpr_<id>`. Avoids a
                        // dynamic-ctor-dispatch substrate change.
                        self.class_value_aliases.get(&n).cloned().unwrap_or(n)
                    }
                    // P8.5 — `new class { ... }(args)` /
                    // `new class Foo { ... }(args)`. Parse the class
                    // expression inline as a ClassDecl with synth name,
                    // buffer to synth_classes, and use the synth name
                    // as the new target. parse_class_decl_with_abstract
                    // consumes `class` + body itself.
                    Token::Class => {
                        let stmt = self.parse_class_decl_with_abstract(false, true, true)?;
                        let cls_name = match &stmt {
                            Stmt::ClassDecl { name, .. } => name.clone(),
                            _ => unreachable!(),
                        };
                        self.synth_classes.push(stmt);
                        cls_name
                    }
                    // P8.5 — `new (expr)(args)` per ES spec §13.3.5
                    // NewExpression: the callee may be a parenthesized
                    // expression. tora handles the case where the inner
                    // expression resolves to an Ident — which covers
                    // `new (class { ... })()` (the class-expression
                    // branch in parse_primary emits an Ident at that
                    // inner site) and `new (SomeClass)()`. Non-Ident
                    // callees (dynamic ctor invocation through a
                    // computed value) require runtime constructor
                    // dispatch — V3-later.
                    Token::LParen => {
                        self.pos += 1;
                        let inner = self.parse_expr()?;
                        match self.peek() {
                            Token::RParen => self.pos += 1,
                            t => {
                                return Err(format!(
                                    "expected `)` after `new (...`, got {t:?} at {}",
                                    self.at()
                                ));
                            }
                        }
                        match self.ast.get_expr(inner) {
                            Expr::Ident(n) => n.clone(),
                            other => {
                                return Err(format!(
                                    "new (expr) callee must resolve to a class ident (got \
                                     {other:?}) at {}",
                                    self.at()
                                ));
                            }
                        }
                    }
                    t => {
                        return Err(format!(
                            "expected class name after `new`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                // V3-18 wedge — accept-and-skip TS type args on built-in
                // generics: `new Set<number>()`. Subset doesn't mono-
                // instantiate built-ins by type-arg yet, so we just
                // discard the `<...>` portion. Depth-aware match in case
                // of nested generics like `Map<string, Array<number>>`.
                if matches!(self.peek(), Token::Lt) {
                    self.pos += 1;
                    let mut depth = 1;
                    while depth > 0 {
                        match self.peek() {
                            Token::Lt => depth += 1,
                            Token::Gt => depth -= 1,
                            Token::ShrShr => depth -= 2,
                            Token::Eof => {
                                return Err(format!(
                                    "unterminated type args after `new {class_name}` at {}",
                                    self.at()
                                ));
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                }
                // V3-18 m1.h.22 — JS spec §13.3.5 NewExpression
                // permits `new Foo` (no parens), equivalent to
                // `new Foo()`. Test262 uses both forms; the no-
                // parens form previously hard-rejected.
                let has_parens = matches!(self.peek(), Token::LParen);
                if has_parens {
                    self.pos += 1;
                }
                let mut args: Vec<ExprId> = Vec::new();
                if has_parens && !matches!(self.peek(), Token::RParen) {
                    args.push(self.parse_expr()?);
                    while matches!(self.peek(), Token::Comma) {
                        self.pos += 1;
                        args.push(self.parse_expr()?);
                    }
                }
                if has_parens {
                    match self.peek() {
                        Token::RParen => self.pos += 1,
                        t => return Err(format!("expected `)`, got {t:?} at {}", self.at())),
                    }
                }
                Ok(self.ast.add_expr(Expr::New { class_name, args }))
            }
            t => Err(format!(
                "expected expression, got {t:?} at {}",
                self.tokens[pos].span.start
            )),
        }
    }

    /// Phase K.1 — `import` declaration parser. Single-file mode: builds
    /// the AST node so the syntax is accepted; the lowerer treats it as
    /// a no-op until K.2 wires in cross-file linking. Recognized shapes:
    ///   - `import "./x"`                       (side-effect-only)
    ///   - `import x from "./x"`                (default import)
    ///   - `import * as ns from "./x"`          (namespace import)
    ///   - `import { a, b as c } from "./x"`    (named imports)
    ///   - `import x, { a, b } from "./x"`      (combined default + named)
    ///   - `import x, * as ns from "./x"`       (combined default + namespace)
    fn parse_import(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `import`
        let mut default: Option<String> = None;
        let mut namespace: Option<String> = None;
        let mut named: Vec<(String, Option<String>)> = Vec::new();
        // Bare `import "./x"` — no clause, just the source.
        if let Token::String(_) = self.peek() {
            let source = match self.peek() {
                Token::String(s) => s.clone(),
                _ => unreachable!(),
            };
            self.pos += 1;
            self.skip_optional_semi();
            return Ok(Stmt::ImportDecl {
                default,
                namespace,
                named,
                source,
            });
        }
        // Default import: `import x ...` (next token is Ident).
        if let Token::Ident(_) = self.peek() {
            let name = match self.peek() {
                Token::Ident(n) => n.clone(),
                _ => unreachable!(),
            };
            self.pos += 1;
            default = Some(name);
            // Optional `, { ... }` or `, * as ns`.
            if matches!(self.peek(), Token::Comma) {
                self.pos += 1;
            }
        }
        // Namespace: `* as ns` (Token::Star + Ident("as") + Ident).
        if matches!(self.peek(), Token::Star) {
            self.pos += 1;
            self.expect_ident_keyword("as")?;
            let n = match self.peek() {
                Token::Ident(n) => n.clone(),
                t => {
                    return Err(format!(
                        "expected namespace ident after `* as`, got {t:?} at {}",
                        self.at()
                    ));
                }
            };
            self.pos += 1;
            namespace = Some(n);
        }
        // Named: `{ a, b as c }`.
        if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            while !matches!(self.peek(), Token::RBrace) {
                let orig = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected ident in import named clause, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                let alias = if matches!(self.peek(), Token::Ident(n) if n == "as") {
                    self.pos += 1;
                    let a = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected alias ident after `as`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    Some(a)
                } else {
                    None
                };
                named.push((orig, alias));
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                }
            }
            self.pos += 1; // consume `}`
        }
        // `from "./x"` tail.
        self.expect_ident_keyword("from")?;
        let source = match self.peek() {
            Token::String(s) => s.clone(),
            t => {
                return Err(format!(
                    "expected string source after `from`, got {t:?} at {}",
                    self.at()
                ));
            }
        };
        self.pos += 1;
        self.skip_optional_semi();
        Ok(Stmt::ImportDecl {
            default,
            namespace,
            named,
            source,
        })
    }

    /// Phase K.1 — `export` declaration parser. Recognized shapes:
    ///   - `export function/class/type/const/let X ...`  (modifier on decl)
    ///   - `export { a, b as c }`                        (named re-export)
    ///   - `export default <expr>`                        (default export)
    fn parse_export(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // consume `export`
        // `export default <expr>;`
        if matches!(self.peek(), Token::Default) {
            self.pos += 1;
            let e = self.parse_expr()?;
            self.skip_optional_semi();
            return Ok(Stmt::ExportDecl {
                inner: None,
                named: Vec::new(),
                default_expr: Some(e),
                source: None,
            });
        }
        // `export { a, b as c };`
        if matches!(self.peek(), Token::LBrace) {
            self.pos += 1;
            let mut named: Vec<(String, Option<String>)> = Vec::new();
            while !matches!(self.peek(), Token::RBrace) {
                let orig = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected ident in export named clause, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                let alias = if matches!(self.peek(), Token::Ident(n) if n == "as") {
                    self.pos += 1;
                    let a = match self.peek() {
                        Token::Ident(n) => n.clone(),
                        t => {
                            return Err(format!(
                                "expected alias ident after `as`, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    };
                    self.pos += 1;
                    Some(a)
                } else {
                    None
                };
                named.push((orig, alias));
                if matches!(self.peek(), Token::Comma) {
                    self.pos += 1;
                }
            }
            self.pos += 1; // consume `}`
            // P13-S4 — optional `from "./b"` for re-export form.
            let source = if matches!(self.peek(), Token::Ident(n) if n == "from") {
                self.pos += 1;
                match self.peek() {
                    Token::String(s) => {
                        let s = s.clone();
                        self.pos += 1;
                        Some(s)
                    }
                    t => {
                        return Err(format!(
                            "expected module source string after `from`, got {t:?} at {}",
                            self.at()
                        ));
                    }
                }
            } else {
                None
            };
            self.skip_optional_semi();
            return Ok(Stmt::ExportDecl {
                inner: None,
                named,
                default_expr: None,
                source,
            });
        }
        // `export <decl>` — modifier on a function / class / type / let
        // / const declaration. Single-file mode just unwraps the inner
        // decl; the AST-level wrapper is preserved for future K.2 work.
        let inner = self.parse_stmt()?;
        Ok(Stmt::ExportDecl {
            inner: Some(Box::new(inner)),
            named: Vec::new(),
            default_expr: None,
            source: None,
        })
    }

    fn expect_ident_keyword(&mut self, kw: &str) -> Result<(), String> {
        match self.peek() {
            Token::Ident(n) if n == kw => {
                self.pos += 1;
                Ok(())
            }
            t => Err(format!("expected `{kw}`, got {t:?} at {}", self.at())),
        }
    }

    fn skip_optional_semi(&mut self) {
        if matches!(self.peek(), Token::Semi) {
            self.pos += 1;
        }
    }

    /// M5.1 — `class C { field: T; constructor(...) {...} method(...): R {...} }`.
    /// Single class, no inheritance / super / static / accessors. Lowered
    /// post-parse by `desugar_classes` into a `TypeDecl` + a series of
    /// `FnDecl`s. The parser only assembles the structure here.
    fn parse_class_decl(&mut self) -> Result<Stmt, String> {
        self.parse_class_decl_with_abstract(false, false, false)
    }

    /// V3-18 wedge — TS parameter-property shorthand
    /// (`constructor(public x: number, private readonly y: string)`).
    /// Returns the regular param list plus a side-table of
    /// (param_index, visibility, is_readonly) entries for params that
    /// should be promoted to instance fields, plus the destr-let vec
    /// for any binding-pattern params (synthesized `__param_destr_<id>`
    /// hidden bindings + per-element / per-field lets to prepend to the
    /// ctor body — caller does the prepend before promoted-prop assigns).
    /// Visibility / readonly modifiers can't combine with a destr
    /// pattern at the same param position (a binding pattern has no
    /// single field-name to promote).
    fn parse_ctor_param_list(
        &mut self,
    ) -> Result<(Vec<Param>, Vec<(usize, ast::Visibility, bool)>, Vec<Stmt>), String> {
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        let mut promoted: Vec<(usize, ast::Visibility, bool)> = Vec::new();
        let mut destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // Consume any TS modifiers: visibility (`public` /
                // `private` / `protected`) and `readonly`. Order is
                // visibility-then-readonly per TS, but we accept any
                // combination once.
                let mut vis: Option<ast::Visibility> = None;
                let mut rd = false;
                loop {
                    let Token::Ident(s) = self.peek() else { break };
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
                let is_rest = matches!(self.peek(), Token::DotDotDot);
                if is_rest {
                    self.pos += 1;
                }
                if !is_rest && matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    if vis.is_some() || rd {
                        return Err(format!(
                            "ctor destructuring param can't carry visibility / readonly modifiers at {}",
                            self.at()
                        ));
                    }
                    let synth = self.parse_destr_param(&mut destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    params.push(Param {
                        name: synth,
                        type_ann,
                        default: None,
                        is_rest: false,
                    });
                    match self.peek() {
                        Token::Comma => {
                            self.pos += 1;
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            continue;
                        }
                        Token::RParen => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `)` after destr ctor param, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
                let pname = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected parameter name, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                let optional = !is_rest && matches!(self.peek(), Token::Question);
                if optional {
                    self.pos += 1;
                }
                let type_ann = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    let ann = self.parse_type_ann()?;
                    if optional && !ann.starts_with("__nullable(") {
                        Some(format!("__nullable({ann})"))
                    } else {
                        Some(ann)
                    }
                } else {
                    None
                };
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else if optional {
                    // Implicit null default for `name?: T` (mirrors parse_fn).
                    Some(self.ast.add_expr(Expr::Null))
                } else {
                    None
                };
                let idx = params.len();
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest,
                });
                if vis.is_some() || rd {
                    promoted.push((idx, vis.unwrap_or(ast::Visibility::Public), rd));
                }
                match self.peek() {
                    Token::Comma => {
                        if is_rest {
                            return Err(format!("rest parameter must be last at {}", self.at()));
                        }
                        self.pos += 1;
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in params, got {t:?} at {}",
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
        Ok((params, promoted, destr_lets))
    }

    /// Shared helper: parse a `(p1: T, p2: T, ...)` parameter list.
    /// Used by class methods/ctors. (Existing `parse_fn` / `parse_arrow_fn`
    /// have their own copies inlined; not refactoring them here to keep the
    /// M5.1 diff focused.)
    /// V3-18 wedge — return `(params, destr_lets)`. Destr_lets is the
    /// vec of `let bound = synth.field` (or `synth[i]`) statements
    /// generated when one or more params are binding patterns rather
    /// than identifiers. The caller is responsible for prepending
    /// destr_lets to the parsed body. When no destr params appear,
    /// destr_lets is empty and the caller's prepend is a no-op.
    fn parse_param_list(&mut self) -> Result<(Vec<Param>, Vec<Stmt>), String> {
        match self.peek() {
            Token::LParen => self.pos += 1,
            t => return Err(format!("expected `(`, got {t:?} at {}", self.at())),
        }
        let mut params = Vec::new();
        let mut param_destr_lets: Vec<Stmt> = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                // Rest parameter: `...name`. Must be the last param;
                // the post-loop check enforces it.
                let is_rest = matches!(self.peek(), Token::DotDotDot);
                if is_rest {
                    self.pos += 1;
                }
                if !is_rest && matches!(self.peek(), Token::LBracket | Token::LBrace) {
                    let synth = self.parse_destr_param(&mut param_destr_lets)?;
                    let type_ann = if matches!(self.peek(), Token::Colon) {
                        self.pos += 1;
                        Some(self.parse_type_ann()?)
                    } else {
                        None
                    };
                    // P-PARSE.6 — whole-pattern default on a destr
                    // method param: third call site for the same
                    // destr-default plumbing (parse_fn / parse_arrow_fn
                    // / class-method parse_param_list).
                    let default = if matches!(self.peek(), Token::Eq) {
                        self.pos += 1;
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    params.push(Param {
                        name: synth,
                        type_ann,
                        default,
                        is_rest: false,
                    });
                    match self.peek() {
                        Token::Comma => {
                            self.pos += 1;
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            continue;
                        }
                        Token::RParen => break,
                        t => {
                            return Err(format!(
                                "expected `,` or `)` after destr param, got {t:?} at {}",
                                self.at()
                            ));
                        }
                    }
                }
                let pname = match self.peek() {
                    Token::Ident(n) => n.clone(),
                    t => {
                        return Err(format!(
                            "expected parameter name, got {t:?} at {}",
                            self.at()
                        ));
                    }
                };
                self.pos += 1;
                // V3-18 wedge — optional parameter `name?: T`. Mirrors
                // the parse_fn version so class methods accept the
                // same shape.
                let optional = !is_rest && matches!(self.peek(), Token::Question);
                if optional {
                    self.pos += 1;
                }
                let type_ann = if matches!(self.peek(), Token::Colon) {
                    self.pos += 1;
                    let ann = self.parse_type_ann()?;
                    if optional && !ann.starts_with("__nullable(") {
                        Some(format!("__nullable({ann})"))
                    } else {
                        Some(ann)
                    }
                } else {
                    None
                };
                // Default value: `= <expr>`. Evaluated at the call
                // site (not in callee scope) when the caller omits
                // the arg. Not allowed on rest params. Optional `name?: T`
                // without an explicit default gets the implicit `null`
                // default (subset's undefined sentinel), matching parse_fn.
                let default = if !is_rest && matches!(self.peek(), Token::Eq) {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else if optional {
                    Some(self.ast.add_expr(Expr::Null))
                } else {
                    None
                };
                params.push(Param {
                    name: pname,
                    type_ann,
                    default,
                    is_rest,
                });
                match self.peek() {
                    Token::Comma => {
                        if is_rest {
                            return Err(format!("rest parameter must be last at {}", self.at()));
                        }
                        self.pos += 1;
                        // V3-18 wedge — trailing comma in param list,
                        // per JS spec §13.3.3 ('function f(a, b,)'). Detect
                        // immediately-following ')' and break out.
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    }
                    Token::RParen => break,
                    t => {
                        return Err(format!(
                            "expected `,` or `)` in params, got {t:?} at {}",
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
        Ok((params, param_destr_lets))
    }

    fn parse_array_literal(&mut self) -> Result<ExprId, String> {
        // assumes current token is `[`
        self.pos += 1;
        let mut elements = Vec::new();
        // P-PARSE.1 — sparse array literal `[1, , 3]`. A comma in the
        // element position is an elision; per ES spec §13.2.4 it
        // contributes one slot whose value is `undefined`. Pre-fix
        // tora's parser bailed at the comma with 'expected expression,
        // got Comma'. Until P1 ships real Type::Undefined the elision
        // synthesizes an `Expr::Null` placeholder — at the storage
        // layer Nullable<T> is the closest existing shape, and
        // test262 cases that hit sparse arrays mostly check `.length`
        // which is unaffected by the elision-value choice.
        let parse_elem_or_elision = |this: &mut Self| -> Result<ExprId, String> {
            if matches!(this.peek(), Token::Comma | Token::RBracket) {
                return Ok(this.ast.add_expr(Expr::Null));
            }
            this.parse_array_element()
        };
        if !matches!(self.peek(), Token::RBracket) {
            elements.push(parse_elem_or_elision(self)?);
            while matches!(self.peek(), Token::Comma) {
                self.pos += 1;
                if matches!(self.peek(), Token::RBracket) {
                    break; // trailing comma allowed
                }
                elements.push(parse_elem_or_elision(self)?);
            }
        }
        match self.peek() {
            Token::RBracket => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `]` in array literal, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        Ok(self.ast.add_expr(Expr::Array(elements)))
    }

    /// One slot inside an array literal — either a spread `...src` or a
    /// regular expression. Spread is wrapped in `Expr::Spread { expr }`
    /// so ssa_lower's Array arm can fork into the pre-sized alloc path.
    fn parse_array_element(&mut self) -> Result<ExprId, String> {
        if matches!(self.peek(), Token::DotDotDot) {
            self.pos += 1;
            let inner = self.parse_expr()?;
            return Ok(self.ast.add_expr(Expr::Spread { expr: inner }));
        }
        self.parse_expr()
    }

    /// One arg inside a Call expression — same shape as parse_array_element
    /// so `f(...arr)` parses to `f(Expr::Spread { expr: arr })`. The
    /// `apply_rest_args` AST pass handles the call-site lowering.
    fn parse_call_arg(&mut self) -> Result<ExprId, String> {
        if matches!(self.peek(), Token::DotDotDot) {
            self.pos += 1;
            let inner = self.parse_expr()?;
            return Ok(self.ast.add_expr(Expr::Spread { expr: inner }));
        }
        self.parse_expr()
    }
}
