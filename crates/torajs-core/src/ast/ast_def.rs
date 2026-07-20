//! The [`Ast`] arena type and its inherent methods — split out of the
//! parent under the 500-line file discipline (rotation 146), leaving
//! `ast.rs` as the module-tree and re-export face (the same shape
//! `torajs-core/src/lib.rs` settled into at chunk 486).
//!
//! Verbatim move; `pub use ast_def::Ast` in the parent keeps every
//! `crate::ast::Ast` path working.

use super::*;

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
    /// RFC 20260714-dstr-residual blade 2 — ES §8.4.5 NamedEvaluation
    /// for destructuring defaults: `[fn = function () {}]` names the
    /// anonymous definition by its binding identifier. The desugar
    /// wraps the default in a ternary, so pass-2B's LetDecl-init walk
    /// can't see it; the parser records the pair here by the default's
    /// `ArrowFn` ExprId at pattern-parse time instead.
    pub dstr_default_names: std::collections::HashMap<ExprId, String>,
    /// Lifted-closure name (`__closure_N`) → destructuring binding
    /// name, translated from `dstr_default_names` by `lift_arrow_fns`;
    /// merged into pass-2B's registry BEFORE the self-name overlay
    /// (a named fn expression's self-name still wins per §15.5.5).
    pub closure_dstr_names: std::collections::HashMap<String, String>,
    /// RFC 20260714-dstr-residual blade 3 — every array binding pattern
    /// reads its elements out of a `__ary_src_<id>` group temp, and the
    /// temp's init ExprId is the key here. The value is the pattern's
    /// step budget: how many elements the pattern names before its rest
    /// (`-1` when it HAS a rest, which drains to exhaustion). The
    /// checker consults this to pick the group's lane and the lowerer
    /// to size the iterator walk.
    pub ary_destr_groups: std::collections::HashMap<ExprId, i64>,
    /// The `ary_destr_groups` subset whose source is not a statically
    /// indexable container — a generator, a Map / Set, a class instance
    /// with `[Symbol.iterator]()`, or anything behind `any`. Index reads
    /// are wrong for those (ES §13.15.5.3 destructures through the
    /// iterator protocol), so the lowerer materializes the source's
    /// first `limit` steps into an `Array<Any>` and the pattern reads
    /// that instead.
    ///
    /// Decided by the checker (only it knows the source's type) and
    /// parked here by `check_monomorph` on the owned post-check AST, so
    /// the lowerer reads it straight off `ctx.ast`. Empty pre-check.
    pub iter_destr_srcs: std::collections::HashMap<ExprId, i64>,
    /// RFC 20260714-dstr-residual blade 4 — NamedEvaluation for
    /// anonymous class expressions: synth class name
    /// (`__ClassExpr_<id>`) → binding identifier, recorded by the
    /// parser at `let D = class {}` bindings and destructuring
    /// defaults. `synthesize_class_globals` uses it as the `.name`
    /// field of the class value global. A named class expression
    /// (`class Inner {}`) parses under its own name and never
    /// enters this map (§15.5.5: the self-name wins).
    pub class_expr_display_names: std::collections::HashMap<String, String>,
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
    /// RFC 20260718-error-message-own-prop 刀 3 — classes whose ctor
    /// the USER spelled out (`constructor(...) {...}` parsed). The
    /// no-super ReferenceError append keys on this: a parser-synth
    /// field-init ctor / a desugar default ctor is not a user ctor
    /// and never gets the raiser.
    pub explicit_ctor_classes: std::collections::HashSet<String>,
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
    /// RFC 20260718-accessor-reify 刀 3 — static-accessor twins of
    /// the instance maps above (`(ClassName, prop) → __sm_<C>__<p>_get
    /// / _set`). Reads/writes were already rewritten to face calls in
    /// desugar; class_globals reads these to emit the reify magic
    /// (AccessorPair own entry on the class object) and to keep the
    /// face FnDecls out of the static-METHOD reify sweep.
    pub static_accessor_getters: std::collections::HashMap<(String, String), String>,
    pub static_accessor_setters: std::collections::HashMap<(String, String), String>,
    /// RFC 20260708-closure-argc-abi — bindings that hold a
    /// length-only real-argc closure VALUE and passed the
    /// direct-call-or-alias safety walk. Populated by
    /// `desugar_arguments_object`; the checker's closure-argc call
    /// wedge admits beyond-arity calls through these names and the
    /// ssa_lower closure-local call arm prepends the runtime argc.
    pub closure_argc_locals: std::collections::HashSet<String>,
    /// RFC 20260714-objlit-accessor blade 1 — ExprIds of object-literal
    /// METHOD-shorthand values (`{ m() { ... } }`). The parser desugars
    /// the shorthand to an `Expr::ArrowFn` like any other field value,
    /// which erases the one semantic that separates them: a method binds
    /// `this` to the receiver, an arrow takes the lexical `this`.
    /// `desugar_objlit_nominal` reads this set to give exactly these
    /// closures a `__this` receiver param. Keyed by ExprId, which
    /// survives `lift_arrow_fns` (it replaces the arena slot in place
    /// with the `Expr::Closure`).
    pub objlit_method_exprs: std::collections::HashSet<ExprId>,
    /// RFC 20260717-class-first-class-value fix-up — value ExprIds of
    /// `{ __proto__ }` PROPERTY-SHORTHAND fields. §B.3.1 scopes the
    /// `__proto__: v` [[Prototype]]-set special case to the
    /// `PropertyName : AssignmentExpression` production only — a
    /// shorthand `__proto__` is an ordinary own data property. The
    /// AST flattens both to the same `(String, ExprId)` field shape,
    /// so the parser records the shorthand's value expr here and the
    /// dynobj-init lane skips the proto special-case for it.
    pub objlit_shorthand_proto_exprs: std::collections::HashSet<ExprId>,
    /// RFC 20260718-builtin-error-ctor-first-class 刀 1 — the class
    /// names `inject_builtin_classes` synthesized (Error + the
    /// NativeError subclasses). class_globals emits the
    /// `__torajs_error_proto_install` magic only for these, so a
    /// USER `class MyErr extends Error` keeps its spec prototype
    /// shape (no own `name` / `message` — it inherits them).
    pub injected_error_classes: std::collections::HashSet<String>,
    /// RFC 20260714-objlit-accessor blade 1 — `__ObjLit_<n>` (the synth
    /// nominal alias minted for a method-bearing object literal) → its
    /// method field names. `ssa_lower` resolves the alias to a
    /// `StructId` once and turns this into a `(StructId, field)` set, so
    /// the field-call arm knows to prepend the receiver.
    ///
    /// Keyed by the NOMINAL name on purpose. The class accessor lane
    /// keys the same question by reverse-looking-up a class name from a
    /// structurally-equal alias, which is unsound (a plain `{a:1}` steals
    /// a same-layout class's accessor — see the RFC); this table must not
    /// repeat that.
    pub objlit_method_fields: std::collections::HashMap<String, Vec<String>>,
    /// RFC 20260717-fnexpr-this-channel knife 1 — ExprIds of
    /// non-generator function EXPRESSIONS (`function (v) { ... }` in
    /// expression position). The parser desugars them to `Expr::ArrowFn`
    /// like arrows, which erases the one semantic that separates them: a
    /// function expression binds `this` at the call site, an arrow takes
    /// the lexical `this`. `desugar_fnexpr_this` reads this set to give
    /// exactly the ones sitting in inline accessor-face positions a
    /// `__this` receiver param. Keyed by ExprId, which survives
    /// `lift_arrow_fns` (it replaces the arena slot in place with the
    /// `Expr::Closure`).
    pub fn_expr_exprs: std::collections::HashSet<ExprId>,
    /// Template-substitution `String(...)` wrappers the parser
    /// synthesizes (§13.2.8.5 hint-string order) — keyed by the
    /// synthesized CALLEE Ident's ExprId (fresh per substitution, so
    /// no user-written `String(...)` can collide). The String-call
    /// lowering branches on this: a keyed call runs the §7.1.17
    /// implicit ToString (a Symbol substitution throws TypeError per
    /// §13.2.8.6), while a real `String(sym)` keeps the §22.1.1
    /// SymbolDescriptiveString.
    pub template_str_calls: std::collections::HashSet<ExprId>,
    /// RFC 20260717-fnexpr-this-channel knife 1 — lifted fn names
    /// (`__closure_N`) whose body takes the call-site `this` as its
    /// first declared param after `__env`. `ssa_lower` stamps
    /// `FLAG_CLOSURE_RECV_FIRST` on the env header at the construction
    /// site and the accessor-face lowering marks the AccessorPair kinds
    /// byte `ACC_KIND_RECV`, so receiver-aware invokers put the
    /// receiver in argv[0].
    pub fnexpr_recv_fns: std::collections::HashSet<String>,
    /// RFC 20260717-fnexpr-this-channel knife 2 — exact face ExprIds
    /// (the `Expr::Ident` in an accessor-face position) whose
    /// SINGLE-USE const binding routes a promoted fn-expr. Keyed by
    /// ExprId — a name set would mis-mark shadowed same-name faces.
    /// The compile-time literal-descriptor lowering reads this to
    /// mark the face's kinds byte `ACC_KIND_RECV`; the runtime paths
    /// need nothing (they read the closure header flag).
    pub fnexpr_recv_faces: std::collections::HashSet<ExprId>,
    /// RFC 20260717-fnexpr-this-channel knife 2W cut 2 — const
    /// binding names whose promoted fn-expr closure is ALSO called
    /// directly by its bare name (the face + direct-call mixed
    /// profile). Promotion requires every use of the name to be a
    /// face read or a direct-call callee, so the set drives exactly
    /// one consumer: the closure-local call arm seeds a boxed
    /// `undefined` into the `__this` argv slot (strict-mode
    /// call-site `this`). Name-keyed like `closure_argc_locals` —
    /// safe because only program-wide-unique decls promote.
    pub fnexpr_recv_locals: std::collections::HashSet<String>,
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
    /// Async function-VALUE expressions that stay in expression
    /// position (RFC 20260713 blade 3): `async () => ...` /
    /// `async x => ...` / `async function(){}`. The parser parses
    /// them for real as `Expr::ArrowFn` and records the ExprId here;
    /// `desugar_async` rewrites each marked body in place (return →
    /// Promise.resolve, try/catch → Promise.reject, return type →
    /// Promise<T>) BEFORE `lift_arrow_fns` runs, so the lifted
    /// `__closure_N` is an ordinary Promise-returning closure and the
    /// capture channel works unchanged (unlike the hoist path, which
    /// is capture-free by construction).
    pub async_fn_value_exprs: std::collections::HashSet<ExprId>,
    /// Lifted-closure names minted from a PLAIN `function` expression
    /// (RFC 20260721-builtin-method-reflection 刀 4+9) — recorded by
    /// `lift_arrow_fns` off `fn_expr_exprs` minus the async set. The
    /// closure-env alloc stamps `FLAG_FN_PROTO` so the runtime
    /// materializes the §10.2.5 `.prototype` object; arrows and async
    /// forms stay off (no `prototype` own property per spec).
    pub fn_proto_fns: std::collections::HashSet<String>,
    /// Lifted-closure names minted from an ASYNC function form
    /// (async arrow / `async function(){}` expression) — recorded by
    /// `lift_arrow_fns` off `async_fn_value_exprs`. The closure-env
    /// alloc stamps `FLAG_FN_ASYNC` so `.constructor` reflects
    /// %AsyncFunction% (刀 4 consumes the bit).
    pub fn_async_value_fns: std::collections::HashSet<String>,
    /// Generator factory fn name → desugared `__Gen_<name>` class
    /// name, recorded by `desugar_generators` when it replaces the
    /// `function*` decl with its thin factory (RFC 20260713 blade 5).
    /// Consumers wire the fn-value reflection surface to the class's
    /// existing prototype substrate: `g.prototype` reads the
    /// tag-registered `__proto___Gen_<name>` (identical to what
    /// `Object.getPrototypeOf(g())` answers) and `x instanceof g`
    /// dispatches on the class's tag set per §27.5.3 [[HasInstance]].
    pub generator_factory_classes: std::collections::HashMap<String, String>,
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
    /// RFC 20260719-fn-tostring-source B2 -- byte ranges of every
    /// outermost type annotation / fn type-param list the parser
    /// consumed, in source order. `fn_source_erase` splices these
    /// (plus their leading `:` / `?` / `as`) out of a recorded fn
    /// span to produce the type-erased source text toString answers.
    pub type_ann_spans: Vec<crate::lexer::Span>,
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
