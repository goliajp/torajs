//! AST — arena-allocated. Children referenced by `ExprId(u32)`, not Box.

mod apply_args;
mod apply_spread;
mod apply_spread_hoist;
mod apply_spread_math;
mod apply_spread_push;
mod arguments_object;
mod arguments_object_collect;
mod arguments_object_rewrite;
mod arguments_object_walkers;
mod array_isarray_value;
mod class_globals;
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
mod expr;
mod fill_optional_fields;
mod fold_fromentries;
mod forwarders;
mod forwarders_object;
mod free_vars;
mod hoist_gen_fn_exprs;
mod implicit_generics_infer;
mod infer_closure_params;
mod infer_closure_typevars;
mod inject_builtin_classes;
mod lift_arrow_fns;
mod module_passes;
mod nested_fns;
mod prop_key;
mod prototype_call;
mod sfi_pass;
mod sfi_rewrite;
mod sfi_safe;
mod stmt;
mod super_collect;
mod uninit_let;
mod var_hoist;
pub use apply_args::{apply_default_args, apply_rest_args};
pub use apply_spread::apply_spread_args;
pub use arguments_object::desugar_arguments_object;
pub use array_isarray_value::desugar_array_isarray_value;
pub use class_globals::synthesize_class_globals;
pub use desugar_async::desugar_async;
use desugar_async::{body_ends_in_return, rewrite_returns_for_async};
pub use desugar_classes::desugar_classes;
pub use desugar_generators::desugar_generators;
pub(crate) use desugar_generators::{expand_yield_into_in_stmt, lift_lets_in_stmt};
pub use desugar_variadic_push::desugar_variadic_push;
pub use escape_analyze::escape_analyze_array_literals;
pub use expr::{BinOp, Expr, ExprId, Param, UnaryOp};
pub use fill_optional_fields::fill_optional_fields;
pub use fold_fromentries::fold_fromentries;
pub use forwarders::synthesize_forwarders;
pub use forwarders_object::{synthesize_fn_to_closure_forwarders, tag_struct_field_closure_types};
pub use hoist_gen_fn_exprs::hoist_gen_fn_exprs;
pub(crate) use implicit_generics_infer::{
    AstExprsView, binds_to_params, body_has_value_return, infer_expr_ann_with, infer_return_ann,
    infer_return_ann_seeded,
};
pub use infer_closure_params::infer_anonymous_closure_params;
pub use inject_builtin_classes::inject_builtin_classes;
pub use lift_arrow_fns::lift_arrow_fns;
pub(crate) use lift_arrow_fns::{
    build_factory_body, default_init_for_field, default_init_for_type, is_fn_arr_ann,
    is_fn_like_ann, method_owner_is_in_chain, rewrite_this_in_ann,
};
pub use module_passes::{desugar_builtin_imports, rename_user_main, unwrap_exports};
pub use nested_fns::desugar_nested_fns;
pub use prop_key::{literal_prop_key, number_prop_key};
pub use prototype_call::desugar_prototype_call;
pub use sfi_pass::rewrite_split_for_i_to_iter;
pub use stmt::{
    AccessorKind, ClassCtor, ClassMethod, StaticField, StaticInit, Stmt, SwitchCase, Visibility,
};
pub use uninit_let::desugar_uninit_let;
pub use var_hoist::desugar_var_hoist;

/// Which function-value expression form a `gen_fn_exprs` entry came
/// from. Blade 2 emits `Generator` only; `Async` / `AsyncGenerator`
/// land with the async-expression blades of the same RFC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenFnExprKind {
    Generator,
    Async,
    AsyncGenerator,
}

/// Per-expression payload for `Ast::gen_fn_exprs`: the form kind plus
/// how many parser-synthesized destructuring lets prefix the body
/// (mirrors `gen_param_destr_prefix` for decl-form generators —
/// `hoist_gen_fn_exprs` re-registers the count under the hoisted name
/// so the __Gen ctor gets the eager destructure).
#[derive(Debug, Clone, Copy)]
pub struct GenFnExprInfo {
    pub kind: GenFnExprKind,
    pub destr_prefix: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Ast {
    pub stmts: Vec<Stmt>,
    pub exprs: Vec<Expr>,
    /// Chunk 796 — ES §15.5.5 named function expressions: the parser
    /// records each fn expression's self-name by its `ArrowFn`
    /// ExprId (`let l = function named() {}` — `named` IS the fn's
    /// `name`; the binding position does not override it). Keying by
    /// ExprId is safe: modules parse into one shared arena
    /// (`parse_into`), no renumbering.
    pub fn_expr_self_names: std::collections::HashMap<ExprId, String>,
    /// Lifted-closure name (`__closure_N`) → fn-expression
    /// self-name, translated from `fn_expr_self_names` by
    /// `lift_arrow_fns`; overlaid LAST in pass-2B's NamedEvaluation
    /// registry so the self-name wins over the binding name.
    pub closure_self_names: std::collections::HashMap<String, String>,
    /// Recorded by `desugar_classes` so post-desugar passes (check, ssa_lower)
    /// can resolve `instanceof Parent` on a subclass instance: maps each
    /// declared class name to its parent (None if no `extends`). Empty
    /// before desugar runs and for programs with no class declarations.
    pub class_parents: std::collections::HashMap<String, Option<String>>,
    /// Chunk 812 — classes whose ctor was synthesized by
    /// `synthesize_derived_default_ctors` (ctor-less derived). Their
    /// factory params mirror the ancestor ctor, but per ES the
    /// implicit default ctor is rest-shaped, so `C.length` is 0 —
    /// `synthesize_class_globals` consults this to avoid reporting
    /// the forwarded param count.
    pub derived_default_ctor_classes: std::collections::HashSet<String>,
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
    /// RFC 20260708-closure-argc-abi — bindings that hold a
    /// length-only real-argc closure VALUE and passed the
    /// direct-call-or-alias safety walk. Populated by
    /// `desugar_arguments_object`; the checker's closure-argc call
    /// wedge admits beyond-arity calls through these names and the
    /// ssa_lower closure-local call arm prepends the runtime argc.
    pub closure_argc_locals: std::collections::HashSet<String>,
    /// RFC 20260708-closure-argv-face — lifted closures whose body
    /// reads `arguments[i]` (the full-arguments tier) and whose
    /// value passed the direct-call-or-alias safety walk. These
    /// bodies carry the synthetic `__torajs_real_argc` +
    /// `__torajs_argv` params and a materialized
    /// `__torajs_arguments` local; the checker mints their Function
    /// type rest-tail (`(...args: any[]) => R`) so every call rides
    /// the boxed dual entry (which feeds real argc + argv).
    pub closure_argv_fns: std::collections::HashSet<String>,
    /// RFC 20260708-closure-argv-face — bindings holding a
    /// `closure_argv_fns` value (aliases included). Recorded for
    /// symmetry with `closure_argc_locals`; the SSA variadic
    /// registration rides the checker's rest-tail type instead.
    pub closure_argv_locals: std::collections::HashSet<String>,
    /// Phase L.2 — names of `async function` declarations recorded by
    /// the parser. desugar_async iterates ast.stmts and, for any
    /// FnDecl whose name is in this set, wraps the return value in a
    /// Promise and shifts the surface return type from T to Promise<T>.
    /// Avoids adding an `is_async: bool` to every FnDecl construction
    /// site.
    pub async_fns: std::collections::HashSet<String>,
    /// `async function*` declarations (RFC 20260713 blade 4). The
    /// parser records the intersection HERE instead of `async_fns` so
    /// `desugar_async` never Promise-wraps the generator FACTORY
    /// (spec §27.6: calling an async generator returns the generator
    /// object directly; each next()/return()/throw() returns a
    /// Promise). `desugar_generators` consumes this set and registers
    /// the mangled `__cm___Gen_<name>__{next,return,throw}` methods
    /// into `async_fns`, so the class-method async rewrite
    /// (desugar_classes_emit) gives the step methods their
    /// Promise<__step_*> shape.
    pub async_generator_fns: std::collections::HashSet<String>,
    /// Generator / async function-value EXPRESSIONS parsed for real
    /// (RFC 20260713-generator-fn-value-substrate). Keyed by the
    /// `Expr::ArrowFn` ExprId the parser emitted; `hoist_gen_fn_exprs`
    /// replaces each marked slot with `Expr::Ident("__genexpr_N")` and
    /// appends a top-level FnDecl so the decl-form desugar machinery
    /// (desugar_generators / desugar_async) handles the body. Side
    /// table instead of a new Expr variant: zero blast radius across
    /// the ~24 files matching Expr::ArrowFn (precedent:
    /// `stack_array_literals`, `fn_expr_self_names`).
    pub gen_fn_exprs: std::collections::HashMap<ExprId, GenFnExprInfo>,
    /// Generator decls whose parameter list contained binding
    /// patterns: fn name → count of parser-synthesized destructuring
    /// `let` stmts prefixed to the body (parse_fn's param_destr_lets).
    /// desugar_generators peels exactly this prefix off the generator
    /// body and moves it into the `__Gen_<name>` ctor so parameter
    /// destructuring evaluates eagerly at the factory call (ES §9.2
    /// FunctionDeclarationInstantiation timing) instead of at the
    /// first `next()`.
    pub gen_param_destr_prefix: std::collections::HashMap<String, usize>,
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
