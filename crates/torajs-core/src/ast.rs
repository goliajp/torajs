//! AST — arena-allocated. Children referenced by `ExprId(u32)`, not Box.

mod apply_args;
mod arguments_object;
mod arguments_object_rewrite;
mod array_isarray_value;
mod class_globals;
mod consuming_flow;
mod desugar_async;
mod desugar_classes;
mod desugar_classes_abstract;
mod desugar_classes_dispatch;
mod desugar_classes_emit;
mod desugar_classes_field_inits;
mod desugar_classes_fields;
mod desugar_classes_method_owners;
mod desugar_classes_pass2;
mod desugar_classes_pass3;
mod desugar_classes_statics;
mod desugar_classes_super;
mod desugar_generators;
mod desugar_generators_class;
mod desugar_generators_methods;
mod desugar_generators_prep;
mod desugar_generators_rewrite;
mod desugar_generators_sm;
mod desugar_variadic_push;
mod escape_analyze;
mod forwarders;
mod forwarders_object;
mod free_vars;
mod implicit_generics_infer;
mod infer_closure_params;
mod inject_builtin_classes;
mod lift_arrow_fns;
mod module_passes;
mod nested_fns;
mod prototype_call;
mod sfi_pass;
mod sfi_rewrite;
mod sfi_safe;
mod stmt;
mod super_collect;
mod uninit_let;
mod var_hoist;
pub use apply_args::{apply_default_args, apply_rest_args};
pub use arguments_object::desugar_arguments_object;
pub use array_isarray_value::desugar_array_isarray_value;
pub use class_globals::synthesize_class_globals;
pub use consuming_flow::compute_consuming_params;
pub use desugar_async::desugar_async;
use desugar_async::{body_ends_in_return, rewrite_returns_for_async};
pub use desugar_classes::desugar_classes;
pub use desugar_generators::desugar_generators;
pub(crate) use desugar_generators::{expand_yield_into_in_stmt, lift_lets_in_stmt};
pub use desugar_variadic_push::desugar_variadic_push;
pub use escape_analyze::escape_analyze_array_literals;
pub use forwarders::synthesize_forwarders;
pub use forwarders_object::{synthesize_fn_to_closure_forwarders, tag_struct_field_closure_types};
pub(crate) use implicit_generics_infer::{
    AstExprsView, binds_to_params, body_has_value_return, infer_expr_ann_with, infer_return_ann,
    infer_return_ann_seeded,
};
pub use infer_closure_params::infer_anonymous_closure_params;
pub use inject_builtin_classes::inject_builtin_classes;
pub use lift_arrow_fns::lift_arrow_fns;
pub(crate) use lift_arrow_fns::{
    build_factory_body, default_init_for_field, default_init_for_type, is_fn_like_ann,
    method_owner_is_in_chain, rewrite_this_in_ann,
};
pub use module_passes::{desugar_builtin_imports, rename_user_main, unwrap_exports};
pub use nested_fns::desugar_nested_fns;
pub use prototype_call::desugar_prototype_call;
pub use sfi_pass::rewrite_split_for_i_to_iter;
pub use stmt::{
    AccessorKind, ClassCtor, ClassMethod, StaticField, StaticInit, Stmt, SwitchCase, Visibility,
};
pub use uninit_let::desugar_uninit_let;
pub use var_hoist::desugar_var_hoist;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    /// V3-01 — `**` exponent. Right-associative; precedence above
    /// mul/div/mod. Number ** Number → Number (libm pow);
    /// BigInt ** BigInt → BigInt (square-and-multiply, RangeError
    /// on negative exponent per spec).
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,  // ===
    Neq, // !==
    /// V3-18 m3 — JS loose equality (`==` / `!=`) per §7.2.13
    /// IsLooselyEqual. Different from `Eq` / `Neq` (strict): does
    /// type coercion across Number/Boolean/Null/String pairs and
    /// treats null==undefined as true.
    LooseEq,
    LooseNeq,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr, // signed; JS `>>`
    /// JS `>>>` — unsigned (logical) right shift. Lowered as LLVM
    /// `lshr` rather than `ashr`; the typechecker still treats it as
    /// `Number → Number` (matches arithmetic Shr).
    UShr,
    LAnd, // logical &&  — short-circuits
    LOr,  // logical ||  — short-circuits
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,    // logical !
    Neg,    // arithmetic -
    BitNot, // bitwise ~
    /// V3-18 m1.h.4 — unary `+x` per spec §13.5.4. Equivalent to
    /// `ToNumber(x)` — coerces strings/booleans/null to a Number
    /// without changing sign. Common in test262 for converting
    /// values to Number explicitly.
    Plus,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String),
    String(String),
    Number(f64),
    /// T-25 — BigInt literal. Carries the digits + radix exactly
    /// as the lexer parsed them (e.g. `123n` → digits="123" radix=10;
    /// `0xffn` → digits="ff" radix=16). The runtime parses the
    /// digits at allocation time via `__torajs_bigint_alloc_from_str`.
    BigInt {
        digits: String,
        radix: u32,
    },
    Bool(bool),
    /// `let x;` / `let x: T;` — placeholder init the parser emits when
    /// no `= EXPR` is provided. `desugar_uninit_let` walks the
    /// declaring scope for the first `x = EXPR;` shape, splices that
    /// EXPR into the let's init, and removes the assignment. Anything
    /// that resists rewrite (no follow-up assignment) keeps `Uninit`,
    /// which the typechecker rejects with a clear "declared but never
    /// assigned" message — better than the previous parse-error wall.
    Uninit,
    /// `/pattern/flags` regex literal. Lexer carries the raw pattern
    /// + flag bytes; the parser wraps them here so check.rs can give
    /// a clean roadmap-phase rejection. Actual matching engine is
    /// future work — the typechecker today rejects regex use with a
    /// "regex literals not yet implemented (planned)" message.
    Regex {
        pattern: String,
        flags: String,
    },
    /// `null` — the in-band 0 sentinel for any pointer-shaped slot.
    /// Lowered to `Operand::ConstPtrNull`. Comparable against pointer
    /// values via `=== null` / `!== null` and the implicit `?.`/`??`
    /// shapes.
    Null,
    BinOp {
        op: BinOp,
        left: ExprId,
        right: ExprId,
    },
    /// Unary prefix op — currently just `!` (logical not). M1.5.
    Unary {
        op: UnaryOp,
        expr: ExprId,
    },
    Member {
        obj: ExprId,
        name: String,
    },
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    Assign {
        target: ExprId,
        value: ExprId,
    },
    Index {
        obj: ExprId,
        index: ExprId,
    },
    Array(Vec<ExprId>),
    /// Object literal: `{ x: 1, y: 2 }`. Field order is preserved as written
    /// (matters for struct layout decisions in P2.4.c).
    ObjectLit {
        fields: Vec<(String, ExprId)>,
    },
    ArrowFn {
        params: Vec<Param>,
        return_type: Option<String>,
        body: Vec<Stmt>,
    },
    /// Lifted closure with implicit captures (M2). After `lift_arrow_fns`,
    /// each capturing arrow becomes this expression: `fn_name` references
    /// the lifted top-level FnDecl (which expects an extra hidden first
    /// param `__env`); `captures` lists the outer-scope binding names that
    /// must be packed into the env at construction time, in the same order
    /// as the lifted FnDecl reads them. Non-capturing arrows still lower
    /// to `Expr::Ident` (FnAddr) — only capturing ones use this variant.
    Closure {
        fn_name: String,
        captures: Vec<String>,
    },
    /// M5.1 — `this` inside a class method body. Rewritten by the
    /// `desugar_classes` pass into `Expr::Ident("__this")` once methods
    /// are flattened into top-level FnDecls.
    This,
    /// P4.5 — `new.target` meta-property (spec §13.3.10). Inside a
    /// ctor body, evaluates to the class function object that was
    /// invoked via `new` (typically the same class as the ctor's
    /// owner, but a subclass if the ctor was inherited / super-called).
    /// Outside any ctor body, evaluates to `undefined`. Rewritten by
    /// `desugar_classes` into `Expr::Ident("__new_target")` inside ctor
    /// bodies; outside ctors it lowers to `Operand::ConstPtrNull` (the
    /// ANY_UNDEF tag at the SSA layer).
    NewTarget,
    /// M5.1 — `new ClassName(args)`. Rewritten by `desugar_classes` into
    /// a Call to the synthesized `__new_ClassName` factory FnDecl.
    New {
        class_name: String,
        args: Vec<ExprId>,
    },
    /// M5.2 — `super(args)` inside a subclass constructor. Rewritten by
    /// `desugar_classes` into `__cm_<Parent>__ctor(__this, args)` once
    /// the surrounding class's parent is known.
    Super {
        args: Vec<ExprId>,
    },
    /// `cond ? then_branch : else_branch` — TS / JS ternary.
    /// Lowered to a CondBr at SSA layer with a phi-style result via
    /// an alloca slot (consistent with how the rest of tr handles
    /// branch results today).
    Ternary {
        cond: ExprId,
        then_branch: ExprId,
        else_branch: ExprId,
    },
    /// V3-18 m1.h.6 — comma operator `(a, b)` per spec §13.16.
    /// Evaluates `left` for side effects (value discarded), then
    /// returns `right`. Result type = right's type.
    Sequence {
        left: ExprId,
        right: ExprId,
    },
    /// V3-07 — `expr as T` TS type cast. Parser-level type assertion;
    /// at runtime the cast is identity (the inner value's bits flow
    /// through). Typecheck uses `ty_ann` to widen / narrow the inner
    /// expression's type — the common shape is `as any` for widening
    /// into a heterogeneous container slot. Currently lowered as
    /// `inner` with the cast type honored only when widening to
    /// `Type::Any` (via the existing Any-box machinery); other casts
    /// are typecheck-only with no IR side effect.
    As {
        expr: ExprId,
        ty_ann: String,
    },
    /// `typeof x` — produces a string literal at runtime.
    /// Lowered to a fresh Type::Str whose contents are determined by
    /// the operand's static type ("number" / "string" / "boolean" /
    /// "object").
    TypeOf {
        expr: ExprId,
    },
    /// `expr instanceof ClassName` — compile-time class membership check.
    /// tr is statically typed: if `expr`'s declared type is the named
    /// class (or a subclass via `extends`), this lowers to ConstBool(true);
    /// otherwise ConstBool(false). The check itself never runs at
    /// runtime — desugar_classes records the class hierarchy, and check.rs
    /// resolves the answer during typechecking.
    InstanceOf {
        expr: ExprId,
        class_name: String,
    },
    /// `...expr` — array spread. Only valid as a child of `Expr::Array`.
    /// ssa_lower's Array arm pre-computes total length (sum of spread
    /// source `.length`s + non-spread element count) at runtime, allocs
    /// once, fills via `arr_push_unchecked` — no cap-doubling realloc.
    Spread {
        expr: ExprId,
    },
    /// `lhs ?? rhs` — nullish coalescing. ssa_lower stores `lhs` into a
    /// temp slot, compares the slot value against null, and branches —
    /// `lhs` evaluates exactly once even if it has side effects.
    Nullish {
        lhs: ExprId,
        rhs: ExprId,
    },
    /// `obj?.field` — optional chaining for member access. ssa_lower
    /// stores `obj` in a temp, branches on null, returns null on the
    /// null path or `obj.field` otherwise. Single eval of `obj`.
    OptChain {
        obj: ExprId,
        name: String,
    },
    /// `x++` / `x--` — JS-spec-compliant post-increment / post-decrement.
    /// Yields the OLD value, then mutates the target. ssa_lower captures
    /// `target`'s value into a temp SSA value, computes new = old ± 1,
    /// stores new into target, and returns the temp.  Pre-increment is
    /// the simpler `x = x + 1` shape and is already handled by Assign.
    PostIncr {
        target: ExprId,
        is_inc: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_ann: Option<String>,
    /// Default value expression: `function f(x: number = 0)`. Evaluated
    /// at the call site (not in callee scope) when the caller omits
    /// the argument. None for required params.
    pub default: Option<ExprId>,
    /// Rest parameter: `function f(...args: number[])`. Only valid as
    /// the last param. The receiver sees `args` as an Array<T>; the
    /// `apply_rest_args` AST pass packs trailing call-site args into
    /// an array literal at every call site.
    pub is_rest: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Ast {
    pub stmts: Vec<Stmt>,
    pub exprs: Vec<Expr>,
    /// Recorded by `desugar_classes` so post-desugar passes (check, ssa_lower)
    /// can resolve `instanceof Parent` on a subclass instance: maps each
    /// declared class name to its parent (None if no `extends`). Empty
    /// before desugar runs and for programs with no class declarations.
    pub class_parents: std::collections::HashMap<String, Option<String>>,
    /// Phase H.3.b — method name → declaring classes in source order
    /// (deepest sub last). Used by ssa_lower's `__dispatch_<M>` Call
    /// interception to emit the runtime tag-switch and call the right
    /// owner's `__cm_<C>__M`. Single-owner methods aren't kept here
    /// since they go through static `__cm_<Owner>__M` dispatch directly.
    pub method_owners: std::collections::HashMap<String, Vec<String>>,
    /// Name-based class-method rewrites desugar made speculatively,
    /// `call ExprId → alt ExprId` where alt clones the original
    /// member-call shape. check.rs demotes a rewrite (types the alt
    /// instead) when the receiver checks as a builtin container —
    /// full mechanism in cm_demote.rs.
    pub speculative_cm_rewrites: std::collections::HashMap<ExprId, ExprId>,
    /// T-24 — virtual method index. Populated only for `chain_methods`
    /// (methods with multiple owners forming a single inheritance
    /// chain — the override case that goes through `__dispatch_<M>`).
    /// Each chain method gets a stable u32 slot in the per-class
    /// vtable; ssa_lower's dispatch interception loads
    /// `vtable_ptr[method_index] -> fn_ptr` and `CallIndirect`s. The
    /// indices are deterministic (sorted by method name) so codegen
    /// is reproducible across builds.
    pub method_index: std::collections::HashMap<String, u32>,
    /// M-OO.5 — `(class_name, member_name)` → visibility, populated by
    /// the parser when a `private` / `protected` modifier appears on a
    /// field or method. Public is the absent-default (no entry stored).
    /// `static_fields` and `static_methods` get the same treatment —
    /// the entry's `class_name` is the class's own name regardless of
    /// instance-vs-static. check.rs reads this map at every Member
    /// access site to enforce the modifier.
    pub member_visibility: std::collections::HashMap<(String, String), Visibility>,
    /// M-OO.5 — `(class_name, field_name)` set of `readonly` fields.
    /// Both instance and static fields can be readonly. check.rs rejects
    /// `obj.field = ...` (instance) and `Class.field = ...` (static)
    /// when the entry is present. Readonly inside the constructor /
    /// class init context is allowed; check.rs's caller-context tracking
    /// lifts the restriction for the same path that visibility uses.
    pub readonly_fields: std::collections::HashSet<(String, String)>,
    /// P8.2 — accessor descriptor side-channel maps (ES §10.1.7).
    /// `(class_name, property_name) → return_type` for getters and
    /// `(class_name, property_name) → param_type` for setters. Populated
    /// by `desugar_classes` while walking ClassDecl members tagged
    /// with `accessor_kind = Some(...)`. The desugared FnDecl is named
    /// `__cm_<class>__<prop>_get` / `__cm_<class>__<prop>_set` so it
    /// can't collide with a same-named regular method or with a
    /// pair of get/set on the same property. check.rs reads the
    /// getter map at Member type resolution (read side) and the
    /// setter map at Assign-Member typecheck (write side); ssa_lower
    /// reads both to emit a Call to the synthesised method instead of
    /// a direct field Load / Store at offset.
    pub accessor_getters: std::collections::HashMap<(String, String), String>,
    pub accessor_setters: std::collections::HashMap<(String, String), String>,
    /// Phase L.2 — names of `async function` declarations recorded by
    /// the parser. desugar_async iterates ast.stmts and, for any
    /// FnDecl whose name is in this set, wraps the return value in a
    /// Promise and shifts the surface return type from T to Promise<T>.
    /// Avoids adding an `is_async: bool` to every FnDecl construction
    /// site.
    pub async_fns: std::collections::HashSet<String>,
    /// Ownership pass — per-function bitmap of which params get
    /// "consumed" (transferred) by the call site instead of
    /// borrowed. A param consumes if its body passes the param into a
    /// `__new_*` constructor factory (which stores it into a class
    /// field) or into another fn already known to consume that
    /// position. Computed by `compute_consuming_params` after all
    /// desugars; check.rs / ssa_lower consult this map at call sites
    /// to decide whether to mark the caller's binding as moved.
    /// Without this, `let g = make_iter(arr); ... drop` creates a
    /// double-free because both `arr` and `g`'s field own the same
    /// heap.
    pub consuming_params: std::collections::HashMap<String, Vec<bool>>,
    /// Plan A — Array literal ExprIds that the escape verifier proved
    /// safe to emit on the stack instead of `__torajs_arr_alloc_pooled`.
    /// Populated by `escape_analyze_array_literals`. ssa_lower's
    /// `Expr::Array` arm checks membership and switches between the
    /// AllocaBytes path (stack) and the heap path. Empty before the
    /// pass runs and for programs with no qualifying literals.
    pub stack_array_literals: std::collections::HashSet<ExprId>,
    /// v0.3 #4 DWARF — per-Expr source byte ranges. Indexed by
    /// ExprId.0; `Span { start: 0, end: 0 }` is the sentinel for
    /// "not set" (parser fills these on key Expr-emit sites; fallback
    /// chain at panic time uses the nearest enclosing set-span when
    /// a leaf has none). Source-buffer `source` lets the byte-range
    /// translate to (line, col) on demand.
    pub expr_spans: Vec<crate::lexer::Span>,
    /// Original source text for the file this Ast was parsed from.
    /// Empty before the parser fills it. Used by `byte_to_line_col`
    /// to derive DWARF DILocation values without re-reading files.
    pub source: String,
    /// Cached newline byte offsets, lazily built on first
    /// `byte_to_line_col` call. Empty before that.
    pub newline_offsets: Vec<u32>,
}

/// Thin wrapper preserving the `ast::desugar_builtin_new` public
/// surface for `torajs-cli` callers; body moved into
/// `ast_desugar_builtin_new` sibling (chunk 139).
pub fn desugar_builtin_new(ast: &mut Ast) {
    crate::ast_desugar_builtin_new::run(ast);
}

/// P4.4 — `Function.prototype.bind / call / apply` desugar pass.
/// Rewrites Call exprs whose callee is `Member { obj: Ident(f), name: m }`
/// (m ∈ {"bind", "call", "apply"}) into substrate-correct shapes that
/// piggyback on tora's existing FnSig / Closure ABIs. Source fn must
/// be a top-level FnDecl Ident — typed-tier substrate only. thisArg
/// is silently discarded (tora has no `this` for top-level fns;
/// class-method `this` is a separate substrate, not in P4.4 scope).
///
///   .call(thisArg, a1, a2)       →  f(a1, a2)
///   .apply(thisArg, [a1, a2])    →  f(a1, a2)            [array-lit only]
///   .bind(thisArg, p0, p1)       →  __bind_create_<id>(p0, p1)
///
/// `__bind_create_<id>` is a synthesized factory FnDecl: takes
/// partials as positional params, returns an arrow that captures
/// them. The arrow's body forwards to the source fn with
/// (partials..., remaining_args...). The capture mechanism is the
/// existing lift_arrow_fns substrate — the arrow inside the factory
/// gets lifted automatically.
///
/// Subset limits at this phase:
///   - `.apply` with non-literal array → left as-is, fails later
///     with informative typecheck error (full dynamic spread is
///     P5.5).
///   - `.bind` source must be Ident referring to a known FnDecl
///     (closure or runtime-Any source is post-P4.4 substrate).
///   - thisArg is silently dropped — `this` semantics arrive with
///     full class substrate (post-P4 / P8 class spec).
///
/// Runs AFTER `lift_arrow_fns` / `synthesize_*_forwarders` so the
/// synthesized factory's inner arrow goes through the normal
/// closure-lift pipeline on the SECOND lift pass... actually we
/// need to be careful here — once lift_arrow_fns has run, new
/// arrows we emit won't be lifted. Solution: emit the FACTORY's
/// inner arrow as a SEPARATE pre-lifted FnDecl `__bound_<id>`
/// with explicit `__env(p0|p1|...)` annotation, and the factory
/// body returns `Expr::Closure { fn_name: "__bound_<id>",
/// captures: ["p0", "p1", ...] }` directly. Matches the post-
/// lift_arrow_fns invariant.
/// Thin wrapper preserving the `ast::desugar_function_prototype_methods`
/// public surface for `torajs-cli` callers (main / lsp / repl /
/// cmd_build); body moved into `ast_desugar_function_prototype_methods`
/// sibling (chunk 138).
pub fn desugar_function_prototype_methods(ast: &mut Ast) {
    crate::ast_desugar_function_prototype_methods::run(ast);
}

/// Thin wrapper preserving the `ast::desugar_implicit_generics`
/// public surface for `torajs-cli` callers; body moved into
/// `ast_desugar_implicit_generics` sibling (chunk 140).
pub fn desugar_implicit_generics(ast: &mut Ast) {
    crate::ast_desugar_implicit_generics::run(ast);
}

impl Ast {
    pub fn add_expr(&mut self, e: Expr) -> ExprId {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(e);
        // Sentinel span until parser (or a desugar pass) sets a real
        // one via `set_expr_span`. Both fields zero means "unknown".
        self.expr_spans
            .push(crate::lexer::Span { start: 0, end: 0 });
        id
    }

    /// v0.3 #4 — record the source byte range of `eid`'s originating
    /// token (or sub-token range). Idempotent in the sense that
    /// later calls overwrite earlier ones; parser is the canonical
    /// caller, but desugar passes that emit synthetic Exprs may also
    /// inherit a span from their originating user node.
    pub fn set_expr_span(&mut self, eid: ExprId, span: crate::lexer::Span) {
        if (eid.0 as usize) < self.expr_spans.len() {
            self.expr_spans[eid.0 as usize] = span;
        }
    }

    /// Build the newline offset table once. Idempotent. Call this
    /// after setting `self.source`; subsequent `byte_to_line_col`
    /// lookups become `&self`-only and can be invoked from
    /// borrow-restricted contexts (codegen, etc).
    pub fn warm_newline_cache(&mut self) {
        if !self.newline_offsets.is_empty() || self.source.is_empty() {
            return;
        }
        for (i, b) in self.source.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                self.newline_offsets.push(i as u32);
            }
        }
    }

    /// Translate a source byte offset into a (line, col) pair, both
    /// 1-indexed (DWARF / editor convention). Returns (0, 0) if the
    /// offset is past end-of-source (sentinel for "no location").
    /// Requires `warm_newline_cache` to have been called when source
    /// is non-empty; otherwise returns (0, 0) for non-zero offsets.
    pub fn byte_to_line_col(&self, byte: u32) -> (u32, u32) {
        if byte == 0 || (byte as usize) > self.source.len() {
            return (0, 0);
        }
        let nl = &self.newline_offsets;
        if nl.is_empty() && !self.source.is_empty() {
            // Cache not warmed; line 1, col=byte+1 as fallback.
            return (1, byte + 1);
        }
        let line = match nl.binary_search(&byte) {
            Ok(k) => (k as u32) + 1,
            Err(k) => (k as u32) + 1,
        };
        let line_start = if line == 1 {
            0u32
        } else {
            nl[(line - 2) as usize] + 1
        };
        let col = byte - line_start + 1;
        (line, col)
    }

    pub fn get_expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.0 as usize]
    }

    pub fn print(&self) {
        for s in &self.stmts {
            crate::ast_printer::print_stmt(self, s, 0);
        }
    }
}
