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

mod arrow_fn;
mod class_field_early_errors;
mod class_member;
mod cursor;
mod destr_defaults;
mod destr_helpers;
mod destr_shape;
mod dstr_assign;
mod expr_entry;
mod expr_prec;
mod fn_expr;
mod forof_binding;
mod import_export;
mod keyword_property;
mod loops;
mod object_literal;
mod object_literal_computed;
mod object_member;
mod object_member_generator;
mod param_list;
mod param_list_early_errors;
mod param_optional_default;
mod parse_class_decl;
mod parse_class_decl_computed_field;
mod parse_class_decl_generator;
mod parse_class_decl_header;
mod parse_class_decl_member;
mod parse_class_member_field;
mod parse_class_member_method;
mod parse_fn;
mod parse_postfix;
mod parse_stmt;
mod parse_yield;
mod primary;
mod primary_async;
mod primary_atoms;
mod primary_new_super;
mod ret_throw_try;
mod try_parse_for_of;
mod type_ann;
mod type_ann_fn;
mod type_ann_helpers;
mod type_decl;
mod yield_expr_hoist;
use class_member::ClassMemberModifierPrefix;
use type_ann_helpers::{is_identifier_name, unwrap_generator_return_ann};

pub fn parse(source: &str, tokens: &[Spanned]) -> Result<Ast, String> {
    let mut ast = Ast::default();
    parse_into(source, tokens, &mut ast)?;
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
pub fn parse_into(source: &str, tokens: &[Spanned], target: &mut Ast) -> Result<usize, String> {
    let stmt_offset = target.stmts.len();
    let id_offset = target.exprs.len() as u32;
    let taken = std::mem::take(target);
    let mut p = Parser {
        source,
        tokens,
        pos: 0,
        type_close_peel: 0,
        type_ann_depth: 0,
        ast: taken,
        desugar_id: id_offset,
        generator_fns: std::collections::HashMap::new(),
        current_class: None,
        in_gen_class_method: false,
        in_async_gen: false,
        pending_async_fn_expr: false,
        static_this_class: None,
        super_call_allowed: false,
        current_class_has_parent: false,
        synth_classes: Vec::new(),
        class_value_aliases: std::collections::HashMap::new(),
        dyn_import_counter: 0,
        yield_hoist_buf: Vec::new(),
        yield_hoist_allowed: true,
        in_formal_params: false,
        await_allowed: true,
    };
    let result = p.parse_program();
    *target = p.ast;
    result?;
    Ok(stmt_offset)
}

struct Parser<'a> {
    /// Source bytes as originally read — kept alongside `tokens` so
    /// restricted-production ASI checks (`parse_return`, postfix `++` /
    /// `--` in `parse_postfix`) can scan the whitespace slice between
    /// two adjacent tokens for a LineTerminator per ES §12.9.1.
    /// Tokens carry byte spans but not the "was there a newline
    /// between me and the previous token?" bit, so the check reads
    /// `source[prev_end..cur_start]` and looks for `\n` / `\r` /
    /// U+2028 / U+2029.
    source: &'a str,
    tokens: &'a [Spanned],
    pos: usize,
    // S155 — `Foo<Bar<X>>` lexes the closing `>>` as one ShrShr token;
    // nested type-arg parsing peels it as two virtual `>`s. This counter
    // tracks how many `>`s of the current ShrShr/ShrShrShr at `pos` have
    // been peeled (0 = full token, 1 = one peel left for `>>`, 2 = both
    // peeled for `>>>`). `peek()` consults it so the second `>` shows
    // up as `Gt` for the outer caller. Reset by any non-Gt advance.
    type_close_peel: u8,
    // RFC 20260719-fn-tostring-source B2 -- recursion depth inside
    // parse_type_ann; only the OUTERMOST annotation records its span
    // into `ast.type_ann_spans` (inner ranges nest inside it).
    type_ann_depth: u32,
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
    /// P-SURF S2.1 — set while parsing the body of a class generator
    /// method, which is hoisted to a top-level `function*` taking the
    /// receiver as a parameter. While it is set, `this` mints
    /// `Ident(GEN_RECV_PARAM)` directly instead of `Expr::This`.
    ///
    /// It has to happen here rather than in a desugar pass:
    /// `desugar_generators` runs *before* `desugar_classes` (the pass
    /// that normally performs the `This` → `__this` rewrite), and the
    /// generator desugar rewrites the params it hoists into fields of a
    /// `__Gen_*` class reached through `this`. An `Expr::This` still
    /// standing at that point would therefore be read as the `__Gen_*`
    /// instance rather than the class instance — silently the wrong
    /// receiver. Minting the parameter reference up front keeps the two
    /// apart.
    in_gen_class_method: bool,
    /// F2/F3 (RFC 20260728-gen-forof-yieldstar) — whether the cursor
    /// is inside an `async function*` body. The generic `yield* e`
    /// desugar reads it to stamp `is_await` on its ForOf (F3: the F1
    /// manual-protocol rewrite then drives the delegation with the
    /// §14.7.5.6 await taps), and it gates the J.3 typed lane off —
    /// an async inner generator's next() answers Promise<__step>,
    /// which the typed expansion would read unsettled. Save/restore
    /// at every generator-body parse site, same discipline as
    /// `super_call_allowed`.
    in_async_gen: bool,
    /// One-shot handshake from `primary_async` to `parse_fn_expr`:
    /// the `async` keyword was consumed one token earlier, so the
    /// expression parser cannot see async-ness itself. `take`n at
    /// `parse_fn_expr` entry; combined with its own `*` detection it
    /// scopes `in_async_gen` to exactly that body.
    pending_async_fn_expr: bool,
    /// Expression-position `yield` hoisting (yield_expr_hoist.rs):
    /// `YieldInto` temps minted mid-expression, drained by the
    /// `parse_stmt` wrapper in front of the finished statement.
    yield_hoist_buf: Vec<Stmt>,
    /// False while parsing a conditionally-evaluated sub-expression
    /// (loop cond/step, short-circuit rhs, ternary branch, optional
    /// call, defaults) — expression-position `yield` rejects there.
    yield_hoist_allowed: bool,
    /// True while parsing a formal parameter list (defaults included):
    /// §15.1.2 / §15.8.1 forbid both YieldExpression and
    /// AwaitExpression anywhere inside FormalParameters, for every
    /// function kind. Cleared per-statement by the `parse_stmt`
    /// wrapper so a nested function BODY inside a default is exempt.
    in_formal_params: bool,
    /// §15.8.1 — `await` is only valid at the top level of a module
    /// (init true) and inside async function bodies. Every
    /// function-like body parse replaces this with its own asyncness
    /// and restores on exit; a class static block forces false
    /// (§15.7.1 ClassStaticBlockBody may not contain await).
    await_allowed: bool,
    /// P-SURF S2.37 — name of the class whose STATIC member body we
    /// are currently inside, or None elsewhere. While set, `this`
    /// mints `Ident(<ClassName>)` directly: per ES §15.7.14 a static
    /// method's `this` is the constructor object, and minting the
    /// class name here lets the existing Pass 2.5/3 static-member
    /// rewrite (`Member { obj: Ident(C), name }` → `__sf_/__sm_`)
    /// resolve `this.#x` / `this.m` in static bodies with no new
    /// downstream lane. Minted at parse time for the same reason
    /// `in_gen_class_method` is: `desugar_classes`' blanket
    /// `This → __this` rewrite runs over the whole arena and cannot
    /// tell static bodies apart by then. Static *generator* bodies
    /// keep the GEN_RECV_PARAM mint (checked first in `primary.rs`)
    /// — their receiver already arrives as a parameter. Known shared
    /// limitation with `in_gen_class_method`: a non-arrow `function`
    /// expression nested in the body inherits the mint even though
    /// its dynamic `this` is not the class.
    static_this_class: Option<String>,
    /// P-SURF S2.9 — whether `super(…)` is legal at the cursor. ES
    /// §15.7.1 makes it an early SyntaxError anywhere but a **derived**
    /// class constructor's body, so it is set only there and cleared on
    /// entry to every other function-like body. An arrow inherits it
    /// rather than clearing: `constructor() { (() => super())() }` is
    /// legal. Defaulting to `false` puts the safe failure (one we fail
    /// to refuse) on a missed clear, and confines the dangerous one to
    /// the single site that sets it.
    super_call_allowed: bool,
    /// Whether the class being parsed has an `extends` clause. Tracked
    /// alongside [`Parser::current_class`] and read only to decide
    /// `super_call_allowed` — `super()` in a base class's constructor
    /// is the same early error as `super()` in a method.
    current_class_has_parent: bool,
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

impl Parser<'_> {
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
        self.reject_decl_in_single_stmt(&then_branch, "an if statement")?;
        let else_branch = if matches!(self.peek(), Token::Else) {
            self.pos += 1;
            let e = Box::new(self.parse_stmt()?);
            self.reject_decl_in_single_stmt(&e, "an else clause")?;
            Some(e)
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

    /// M5.1 — `class C { field: T; constructor(...) {...} method(...): R {...} }`.
    /// Single class, no inheritance / super / static / accessors. Lowered
    /// post-parse by `desugar_classes` into a `TypeDecl` + a series of
    /// `FnDecl`s. The parser only assembles the structure here.
    fn parse_class_decl(&mut self) -> Result<Stmt, String> {
        self.parse_class_decl_with_abstract(false, false, false)
    }
}
