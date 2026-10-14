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

use crate::ast::{
    self, Ast, BinOp, ClassCtor, ClassMethod, Expr, ExprId, Param, PropKey, StaticInit, Stmt,
};
use crate::lexer::{self, Spanned, Token};

mod arrow_fn;
mod asi_ends;
mod class_element_name;
pub(crate) mod class_field_early_errors;
pub use class_field_early_errors::global_code_contains_new_target;
mod class_field_inits;
mod class_member;
mod class_private_dup;
mod class_self_heritage;
mod cursor;
mod delete_expr;
mod destr_defaults;
mod destr_emit;
mod destr_helpers;
mod destr_obj_param;
mod destr_param;
mod destr_shape;
mod dstr_assign;
mod dstr_assign_rest;
mod dstr_assign_step;
mod entry;
mod enum_decl;
mod void_expr;
pub use entry::{parse, parse_into, parse_into_eval};
mod expr_entry;
mod expr_prec;
mod expr_prec_short_circuit;
mod fn_expr;
mod fn_kind_split;
mod forof_binding;
mod forof_forin_src;
mod forof_using;
mod gen_arguments_face;
mod import_export;
mod import_export_tokens;
mod import_export_type;
mod keyword_property;
mod loops;
mod module_item;
mod object_literal;
mod object_literal_computed;
mod object_literal_proto;
mod object_member;
mod object_member_generator;
mod param_list;
mod param_list_early_errors;
mod param_list_modifiers;
pub(crate) mod param_optional_default;
mod parse_class_decl;
mod parse_class_decl_computed_field;
mod parse_class_decl_field;
mod parse_class_decl_generator;
mod parse_class_decl_generator_body;
mod parse_class_decl_header;
mod parse_class_decl_member;
mod parse_class_member_field;
mod parse_class_member_method;
mod parse_fn;
mod parse_let_decl;
mod parse_postfix;
mod single_stmt_judge;
pub(crate) mod tagged_template;
use parse_class_member_field::FieldMarker;
pub(crate) use tagged_template::TEMPLATE_OBJECT_CALLEE;
pub(crate) use type_ann::union_combine;
mod await_ident;
mod dstr_assign_slot;
mod if_block;
mod parse_stmt;
mod parse_yield;
mod primary;
mod primary_async;
mod primary_atoms;
mod primary_new_super;
mod private_in;
mod private_refs;
mod program;
mod reserved_ident;
mod ret_throw_try;
mod stmt_contains;
mod strict_directive;
mod strict_reserved;
mod try_parse_for_of;
mod type_ann;
mod type_ann_fn;
mod type_ann_helpers;
mod type_ann_literal;
mod type_assertion;
mod type_decl;
mod using_decl;
mod with_stmt;
mod yield_cond_hoist;
mod yield_expr_hoist;
mod yield_hoist_events;
use class_member::ClassMemberModifierPrefix;
use type_ann_helpers::{is_identifier_name, unwrap_generator_return_ann};

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
    // 552-03 -- true while parsing an ARROW FN's return annotation
    // (the `: T` between the param list's `)` and the body `=>`).
    // Only there is `(composite) => ...` ambiguous: the arrow's own
    // `=>` sits right behind the annotation, so a parenthesized
    // composite type must NOT greedily read as a fn-type parameter
    // list. Everywhere else (param annotations, type alias bodies)
    // the bare-type parameter grammar makes `(X) => R` a fn-type and
    // the greedy read is correct. Saved/restored around the
    // annotation parse; parse_fn_type_ann consults it at depth 1
    // only, so annotations nested inside the return type keep the
    // greedy read.
    in_arrow_ret_ann: bool,
    /// Inside a `type` / `interface` declaration: a literal type reads
    /// as its marker (`parser/type_ann_literal.rs`), not its base.
    keep_literals: bool,
    /// Whether the BoundNames of the pattern being read belong to a
    /// LEXICAL binding — the half of §14.3.1.1 that a pattern reader
    /// cannot see for itself. Default `true` (refuse), so a binding
    /// form nobody marked keeps today's answer. The measurement that
    /// fixes the axis is on `Parser::shorthand_binding_name`.
    pattern_binds_lexically: bool,
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
    /// RFC 20260921 — the `decl_id` of the class body being parsed,
    /// minted right after its header so the members can record their
    /// owner. Saved / restored around a nested class the same way
    /// `current_class` is; 0 outside any class body.
    current_class_decl_id: u32,
    /// Enclosing class-body scope ids, innermost LAST (ids index
    /// `ast.class_private_scopes`). Pushed/popped alongside
    /// `current_class` by `parse_class_decl_with_abstract`; snapshotted
    /// (reversed) into `ast.private_ref_sites` by each deferred `#x`
    /// reference so `resolve_private_refs` can walk out through the
    /// lexical nesting after the whole file has parsed.
    class_stack: Vec<u32>,
    /// K6-1 — `stmt_depth` at the moment each enclosing class body
    /// (innermost LAST) was entered; pushed / popped with
    /// `class_stack`. A class expression minted at a deeper depth
    /// sits inside a statement of a member body, so its declaration
    /// lands beside its use site (`buffer_class_expr`); one at the
    /// same depth sits in a field initializer or a computed key and
    /// keeps the top-level splice.
    class_body_stmt_depths: Vec<u32>,
    /// K6 residue — true while an INSTANCE field's initializer is
    /// being parsed. The initializer runs per construction, inside
    /// the ctor (`finalize_class_field_inits` splices it there), so a
    /// class expression written in it belongs beside that statement
    /// like one written in a method body — not in the top-level
    /// splice, where an outer local or the enclosing class's statics
    /// are out of reach and every instance shares one class.
    in_instance_field_init: bool,
    /// The class declarations buffered while an initializer parsed,
    /// keyed by the initializer's ExprId; the ctor prefix builder
    /// emits them in front of the `this.<f> = init` statement.
    field_init_classes: std::collections::HashMap<u32, Vec<Stmt>>,
    /// §15.7 — true while a ClassHeritage is being parsed. All parts
    /// of a class are strict code and the heritage is one, but the
    /// `class_stack` push that carries that answer for the BODY
    /// happens after this clause, so it needs a bit of its own.
    in_class_heritage: bool,
    /// §14.7.4 [In]-parameter approximation — true while parsing a
    /// C-style for-head init clause, where the `#x in o` production
    /// does not exist (relational `in` would be ambiguous with the
    /// for-in head). `parse_primary_paren` clears it (parentheses
    /// reset [In]); restored on the for-head's exit.
    in_for_init: bool,
    /// §13.14 — whether the expression `parse_logical_or` /
    /// `parse_logical_and` just returned ate a `||` or `&&` AT THIS
    /// LEVEL (a parenthesised one belongs to the nested parse and does
    /// not count). `parse_nullish` reads it the instant the call
    /// returns, which is the only moment it means anything: a
    /// `CoalesceExpressionHead` is a `BitwiseORExpression`, so a bare
    /// logical operator cannot be the head of a `??`.
    bare_logical: bool,
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
    /// r295 — set by the `this`-mint in `primary.rs` while
    /// `in_gen_class_method` is on. The fn-EXPRESSION generator body
    /// (`fn_expr.rs`) reads it after the body parse: a body that
    /// minted the receiver gets a leading `__genrecv: any = undefined`
    /// param (the class-method form's forwarder passes `this`
    /// explicitly, so it never needs the flag or the default).
    gen_recv_minted: bool,
    /// §13.15.1 — arena ids of the `undefined` idents minted by the
    /// `void <literal>` fold in `expr_prec`. The fold makes `void 0`
    /// indistinguishable from the plain `undefined` ident, but as an
    /// assignment TARGET the two differ: a void expression's
    /// AssignmentTargetType is invalid (parse-time SyntaxError), while
    /// `undefined = x` parses and fails at runtime (§6.2.5.6).
    /// A parenthesized object / array literal is the other such node:
    /// `({}) = 1` is no pattern (§13.15.1), though `{}` alone would be.
    /// `reject_invalid_assignment_target` consults this set.
    invalid_target_ids: std::collections::HashSet<u32>,
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
    /// §15.5.5 early error (r290) — whether the cursor is inside ANY
    /// `function*` body (sync or async; `in_async_gen` is the async
    /// subset). `yield` outside one is a parse-time reject: module
    /// code is strict (§16.1) where `yield` is reserved, and the
    /// reject must precede the resolver so a `yield` nested in an
    /// `import(...)` argument fails at parse phase, not on path
    /// resolution. Save/restore at every function-body parse site,
    /// same discipline as `in_async_gen`; arrows inherit (§15.3).
    in_generator: bool,
    /// §11.2.2 — is the cursor inside a function whose own directive
    /// prologue said `"use strict"`, at any depth? Strictness only
    /// accumulates inward, so the bit is armed per parsed body
    /// statement and restored at every function-body parse site, same
    /// discipline as `in_generator`; blocks and loops carry it through
    /// untouched, being no such site. What it is FOR, and why the
    /// answer is written back into the body rather than tabled: the
    /// `strict_directive` module.
    in_strict_fn: bool,
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
    /// RFC 20260820-dstr-deferred-close — set when a destructuring
    /// assignment's expansion recovered a hoisted yield (a default's
    /// or a target's), i.e. the pattern's evaluation can SUSPEND.
    /// `desugar_dstr_assign` reads it to decide whether the element
    /// statements need the deferred-close try/finally wrap.
    dstra_saw_yield: bool,
    /// 刀 D — the desugar ids whose pattern took the deferred-rest
    /// shape (rest element + suspendable pattern): the rest slot
    /// emits the park-drain sequence instead of the `.slice(i)` tail,
    /// and the group's walk limit is the bounded prefix. Top-level
    /// patterns only — a nested pattern's rest keeps the eager tail
    /// (REMAINDER in the RFC).
    dstra_deferred_rest_ids: std::collections::HashSet<u32>,
    /// §13.15.5.5 — expansion ids whose ELEMENTS step the park one at
    /// a time (a Member / Index element target makes the merged walk
    /// observably wrong). Separate from the rest set above because a
    /// pattern can be either, both, or neither.
    dstra_stepwise_ids: std::collections::HashSet<u32>,
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
    /// The grammar's `[Await]` parameter — whether `await` is a
    /// reserved word HERE, which is the question a BINDING position
    /// asks about it. True inside an async body (§15.8.1) and inside a
    /// ClassStaticBlockBody (§15.7.1: ContainsAwait is a Syntax Error
    /// there, even though the word is fine again inside a nested
    /// non-async function).
    ///
    /// Distinct from [`Parser::await_allowed`], which answers a
    /// different question — may an AwaitExpression appear. The two
    /// differ in exactly two places: a program's top level (that one
    /// is `true` so a module may top-level-await, this one `false` so
    /// `var await = 1` stays an ordinary Script-goal declaration) and
    /// a static block (that one `false`, this one `true`). Everywhere
    /// else they move together, which is why each body parse replaces
    /// them on the same line.
    ///
    /// The goal half of the judgement is NOT here: `await` is reserved
    /// under the Module goal too, and the goal bit is stamped after
    /// the parse, so admitted sites go to `ast.await_ident_positions`
    /// for `ast::triage_await_idents`.
    await_reserved: bool,
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
    /// r334 blade 6 — whether a SuperProperty (`super.x` / `super[k]` /
    /// `super.m(...)`) is legal at the cursor. ES §15.7.1 / §15.4.1 make
    /// `super` an early SyntaxError anywhere outside a MethodDefinition
    /// body, a class field initializer, or a class static block — so it
    /// is set for the span of a class body (`parse_class_decl_with_
    /// abstract`) and for object-literal method bodies (which have a
    /// [[HomeObject]]), and cleared on entry to every ordinary function
    /// body (declaration and expression — their own `this` binding cuts
    /// the home chain). Arrows inherit, same as `super_call_allowed`.
    /// Eval text parses with the flag supplied by the eval desugar
    /// (`parse_into_eval`): PerformEval judges super from the
    /// call site's environment, and an illegal one must surface as a
    /// RUNTIME SyntaxError at the eval — which the desugar builds out
    /// of exactly this parse failure.
    super_prop_allowed: bool,
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
    /// scope-tracking — an inner-fn-body `const F = class {}`
    /// overwrites an outer alias of the same name.
    class_value_aliases: std::collections::HashMap<String, String>,
    /// Class expressions minted outside a class body (393-01) or in a
    /// member body's statement (K6-1). Unlike
    /// `synth_classes` these land NEXT TO their use site — the
    /// `parse_stmt` wrapper drains them in front of the finished
    /// statement — so one inside a nested scope stays a nested
    /// ClassDecl and `hoist_nested_classes` gets to ask its real
    /// question (capture-free → lift; capturing → the ES5 lane).
    /// Splicing to the top level, which `synth_classes` still does for
    /// a field initializer / computed key, LOSES that question: the class body's
    /// free names stop resolving anywhere, silently.
    synth_classes_local: Vec<Stmt>,
    /// Depth of the `parse_stmt` call currently on the stack. 0 after
    /// the outermost call returns — that one belongs to
    /// `parse_program`, whose own splice keeps top-level behavior
    /// byte-identical, so the wrapper only wraps when deeper.
    stmt_depth: u32,
    /// Enum declarations seen so far: merging and the type aliases.
    enums: enum_decl::EnumCtx,
}
