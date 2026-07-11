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
mod forof_destr;
mod import_export;
mod loops;
mod object_literal;
mod object_member;
mod param_list;
mod parse_class_decl;
mod parse_class_decl_header;
mod parse_class_decl_member;
mod parse_class_member_field;
mod parse_class_member_method;
mod parse_fn;
mod parse_postfix;
mod parse_stmt;
mod primary;
mod primary_async;
mod primary_atoms;
mod primary_new_super;
mod ret_throw_try;
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

    fn mint_desugar_id(&mut self) -> u32 {
        let id = self.desugar_id;
        self.desugar_id += 1;
        id
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
            Token::Delete => "delete",
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

    /// M5.1 — `class C { field: T; constructor(...) {...} method(...): R {...} }`.
    /// Single class, no inheritance / super / static / accessors. Lowered
    /// post-parse by `desugar_classes` into a `TypeDecl` + a series of
    /// `FnDecl`s. The parser only assembles the structure here.
    fn parse_class_decl(&mut self) -> Result<Stmt, String> {
        self.parse_class_decl_with_abstract(false, false, false)
    }
}
