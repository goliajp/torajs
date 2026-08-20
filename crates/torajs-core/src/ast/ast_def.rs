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
    /// RFC 20260729-fn-value-any V4 刀 2 — hoisted generator-expression
    /// decl name (`__genexpr_N`) → the name a `.name` read must answer.
    /// `hoist_gen_fn_exprs` erases the expression's syntactic position
    /// before pass-2B can see it, so it resolves NamedEvaluation itself
    /// and parks the verdict here; the fn-name registry reads it in
    /// place of the synthetic mint. An anonymous expression in no
    /// naming position lands an EMPTY string (the ES answer), so a
    /// missing key means "not a hoisted generator expression".
    pub genexpr_names: std::collections::HashMap<String, String>,
    /// Private-name lexical scopes, one entry per class body in parse
    /// order (the entry index is the scope id carried on
    /// `Parser::class_stack`). `.0` is the parse-time class name (the
    /// same string declaration-side mangling bakes into
    /// `__priv_<C>__<n>` member names); `.1` is the set of RAW private
    /// names (`m`, not `#m` / not mangled) the body declares. A `#x`
    /// REFERENCE resolves against the innermost enclosing scope that
    /// declares `x` (ES §15.7 private-name lexical scoping — nested
    /// classes see outer names, an inner redeclaration shadows), which
    /// a single-pass parser cannot decide at the reference token
    /// (later members of the same body may still declare it), so
    /// references park a site in `private_ref_sites` and
    /// `resolve_private_refs` rewrites them once the file is parsed.
    pub class_private_scopes: Vec<(String, std::collections::HashSet<String>)>,
    /// Deferred `#x` reference sites: `.0` is the raw private name,
    /// `.1` the enclosing class-scope-id stack at the reference,
    /// innermost FIRST. The parse-time placeholder member name is
    /// `__privu_<site-index>__<raw>`; `resolve_private_refs` rewrites
    /// it to the declaring class's `__priv_<C>__<raw>` (falling back
    /// to the innermost scope when nothing declares it, which keeps
    /// the checker's undeclared-private compile-time reject).
    pub private_ref_sites: Vec<(String, Vec<u32>)>,
    /// RFC 20260804-fn-this-channel knife 2 — `Expr::This` ExprIds
    /// sitting in a STATIC method/accessor body, keyed to the owning
    /// class name. The parser used to mint `Ident(<cls>)` right at the
    /// token (P-SURF S2.37), which erased the receiver concept from
    /// the AST; now it mints `Expr::This` and records the site here,
    /// and `desugar_classes` pass 2 performs the same class-name
    /// rewrite one layer down — mono semantics byte-identical, but a
    /// future receiver-generic twin can rewrite the same site to
    /// `__this` instead (knife 3).
    pub static_this_sites: std::collections::HashMap<ExprId, String>,
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
    /// RFC 20260820-dstr-deferred-close 刀 D — the `ary_destr_groups`
    /// entries whose pattern ends in a rest element AND can suspend
    /// (a yield in the pattern): their walk limit is the BOUNDED
    /// prefix (the rest drains after the suspension, §13.15.5.5
    /// target-first order), so the checker must route them through
    /// the iterator lane even for a statically indexable source —
    /// the desugar already emitted the drain shape, and the typed
    /// index lane would leave the park slot empty.
    pub dstr_deferred_rest: std::collections::HashSet<ExprId>,
    /// Sloppy-goal implicit globals (§9.1.1.4.6, goal-triage family) —
    /// the names whose hoisted `var` declaration the
    /// `sloppy_implicit_globals` pass synthesized from never-declared
    /// assignment targets. These are USER names (sputnik convention
    /// spells them `__x`), so the data-global promote gate's
    /// synthesized-sentinel `__` filter must not localize them — a
    /// named-fn body writing one needs the global slot.
    pub sloppy_implicit_global_names: std::collections::HashSet<String>,
    /// The bare names the sloppy delete triage folded — recorded so
    /// the implicit-globals sibling never synthesizes a `var` for a
    /// name the program deletes (`x = 1; delete x; x` wants the
    /// unresolvable-name ReferenceError, and a var binding is
    /// non-configurable where the implicit global's property must
    /// delete away).
    pub sloppy_deleted_bare_names: std::collections::HashSet<String>,
    /// Bindings minted by object-rest destructuring (`{a, ...rest}`,
    /// §14.3.3.1 RestBindingInitialization) — the rest object's
    /// static Struct shape is the source-minus-omit ANCHOR, but the
    /// object itself is an ordinary extensible object, so a member
    /// read that misses the anchor is a RUNTIME [[Get]]
    /// (§10.1.8.1): the checker admits it (Any) and the lowering
    /// boxes the receiver onto the any-member probe (hit → value,
    /// miss → undefined). Keyed by binding NAME — a same-named
    /// non-rest binding over-admits, which fails safe: the probe
    /// answers the [[Get]] semantics either way.
    pub obj_rest_names: std::collections::HashSet<String>,
    /// RFC 20260730-undeclared-ident — expression-position occurrences
    /// of identifiers that resolve nowhere (§6.2.5.5 GetValue /
    /// §6.2.5.6 PutValue on an unresolvable Reference), ExprId → name.
    /// Reads AND write-position targets share this table — spec-wise
    /// both are the same unresolvable Reference raising the same
    /// ReferenceError; the assign / post-incr lowering lanes consult it
    /// by target eid. The checker types them `Any` and records them
    /// here (a later read of the same eid that DOES resolve unmarks
    /// it, so speculative pre-pass typing under an incomplete scope
    /// self-heals); the lowerer raises a catchable ReferenceError at
    /// the occurrence. Parked by `check_monomorph` on the owned
    /// post-check AST (same ride as `iter_destr_srcs`). Empty
    /// pre-check.
    pub undeclared_reads: std::collections::HashMap<ExprId, String>,
    /// RFC 20260810 — writes to a fn-expression's own self-name
    /// (§15.5.5: an immutable function-env binding; a strict-mode
    /// write — module code always is — raises TypeError at run time,
    /// not a compile reject). The checker marks the TARGET ident eid,
    /// the assign lane raises through the readonly-assign kernel.
    /// Parked by `check_monomorph` on the owned post-check AST (same
    /// ride as `undeclared_reads`). Empty pre-check.
    pub self_name_writes: std::collections::HashSet<ExprId>,
    /// RFC 20260714-dstr-residual blade 4 — NamedEvaluation for
    /// anonymous class expressions: synth class name
    /// (`__ClassExpr_<id>`) → binding identifier, recorded by the
    /// parser at `let D = class {}` bindings and destructuring
    /// defaults. `synthesize_class_globals` uses it as the `.name`
    /// field of the class value global. A named class expression
    /// (`class Inner {}`) parses under its own name and never
    /// enters this map (§15.5.5: the self-name wins).
    pub class_expr_display_names: std::collections::HashMap<String, String>,
    /// Rotation 373 (L3b 373-05) — synth `__ClassExpr_<id>` decls
    /// whose expression site sat INSIDE an enclosing class body (a
    /// method / accessor / field initializer / static block). Their
    /// evaluation is deferred past the enclosing class's definition,
    /// so `class extends Outer` from within Outer's own body is
    /// legal — but the parser's synth flush lands the decl BEFORE
    /// the enclosing ClassDecl stmt, where the M5.2 source-order
    /// check would reject the parent as a forward reference.
    /// `compute_full_fields` resolves members of this set in a
    /// second, dependency-ordered pass instead.
    pub class_expr_deferred: std::collections::HashSet<String>,
    /// Recorded by `desugar_classes` so post-desugar passes (check, ssa_lower)
    /// can resolve `instanceof Parent` on a subclass instance: maps each
    /// declared class name to its parent (None if no `extends`). Empty
    /// before desugar runs and for programs with no class declarations.
    pub class_parents: std::collections::HashMap<String, Option<String>>,
    /// Blade 3 (RFC 20260815-generic-nominal-identity) — each declared
    /// class's own type-parameter NAMES (`Box<T>` → `["T"]`; empty for
    /// a non-generic class). The heritage-argument composition walks
    /// (inherited-accessor substitution, and whatever else maps a
    /// written arg list onto an ancestor's vocabulary) zip these
    /// against `class_parent_type_args` per hop. Recorded by
    /// `desugar_classes` alongside `class_parents`.
    pub class_type_params: std::collections::HashMap<String, Vec<String>>,
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
    /// Classes whose ctor exists only to hold their field
    /// initializers (`class C { v = 5 }`). Such a class has NOT said
    /// what it takes — per §15.7.14 it has the implicit default ctor —
    /// so a derived one still forwards its ancestor's parameters and
    /// still calls super. Recorded positively, not as the complement
    /// of `explicit_ctor_classes`: that set is parser-filled, so the
    /// injected builtin classes never enter it.
    pub field_init_synth_ctors: std::collections::HashSet<String>,
    /// RFC 20260820-ctor-return-override — classes on an inheritance
    /// chain that touches a value-returning constructor (§10.2.2
    /// [[Construct]] step 13). Filled by
    /// `desugar_classes_ctor_return::collect`, which seeds on the
    /// classes whose own ctor body carries a `return <expr>` and then
    /// spreads BOTH ways along the parent links — descendants because
    /// `class C extends Base {}` must adopt what Base returned, and
    /// ancestors so one chain speaks one ctor ABI (`__this: any`,
    /// answering `any`). Only members of this set change shape; every
    /// other class keeps the typed `void` ctor verbatim.
    pub ctor_return_override: std::collections::HashSet<String>,
    /// Classes extending an exotic-instance builtin (`class C extends
    /// Array`), class name → builtin name (RFC 20260730 blade 1).
    /// Their factory mints a REAL exotic cell through the per-builtin
    /// subclass-alloc kernel instead of an ObjectLit, and the checker
    /// types `new C()` as `any` (exotic instances live in the any
    /// world; fields are dict-mode). Recorded by the builtin-heritage
    /// strip, which also rewrites `super(...)` to the builtin's ctor
    /// semantics.
    pub exotic_parent: std::collections::HashMap<String, String>,
    /// Classes whose stripped builtin parent contributes only a
    /// prototype-chain link (ordinary instances): class name →
    /// builtin-proto tag to chain `__proto_<C>` to at module init
    /// (`class C extends Iterator {}` → 15). Recorded by the
    /// builtin-heritage strip; consumed by class_globals_register's
    /// `__torajs_proto_chain_builtin` emit (RFC
    /// 20260730-iterator-global 刀 1).
    pub builtin_proto_heirs: std::collections::HashMap<String, i64>,
    /// Every class whose stripped `extends` parent is a BUILTIN
    /// constructor (exotic and ordinary alike): class name → builtin
    /// name. §15.7.14 class heritage makes the class object's own
    /// [[Prototype]] the parent constructor, and for a builtin parent
    /// that link cannot resolve through `class_name_to_tag` (the
    /// builtin has no class tag) — the register lowering reads this
    /// table instead and hands the builtin-proto tag to the runtime
    /// wire, which chains the class object to the interned builtin
    /// ctor cell so static inheritance (`CP.resolve` on
    /// `class CP extends Promise`) reads through the ordinary
    /// member-get chain walk.
    pub builtin_class_parents: std::collections::HashMap<String, String>,
    /// Type arguments written at a call site, by the ExprId of the
    /// call. `new Box<number>()` states what `T` is; the rewrite to
    /// the synthesized factory turns it into a plain call, and there
    /// is nowhere on a call to carry them, so they are recorded here
    /// and read back by generic inference. Without it a class whose
    /// type parameter appears only in its field types could not be
    /// instantiated at all — inference is argument-driven, and that
    /// call has no arguments to drive it.
    pub call_type_args: std::collections::HashMap<ExprId, Vec<String>>,
    /// Heritage type arguments by SUBCLASS name, as written —
    /// `class Sub extends Box<number>` records `Sub → ["number"]`.
    /// TS §3.7 gives them no runtime effect of their own, but under
    /// monomorphization the subclass inherits the parent's fields
    /// and those field types spell the parent's type params — the
    /// flattening walk substitutes these spellings in (a generic
    /// parent extended with NO written arguments substitutes `any`,
    /// matching the transpiled-JS behavior bun runs).
    pub class_parent_type_args: std::collections::HashMap<String, Vec<String>>,
    /// 刀 3 (RFC 20260815-fn-value-rest-spread) — top-level binding
    /// names whose init is a REST-fn value (`const g = tail` where
    /// `tail` declares `...args`), recorded at the forwarder wrap.
    /// A closure-captured such binding promotes to a global and its
    /// LetDecl never lowers in the calling fn, so the let-decl lane
    /// that fills `variadic_locals` never sees it — the closure-call
    /// lane reads this set for the promoted flavor of the same
    /// boxed-dual-entry dispatch.
    pub variadic_value_bindings: std::collections::HashSet<String>,
    /// 424-04 — MUTABLE fn-typed binding names where some store site
    /// (init or assign rhs) is a top-level FnDecl whose face is NOT
    /// sig-exact against the declared annotation (the S2 excess-Any
    /// admit shapes). The bare closure-call lane fires a
    /// `call_indirect` shaped by the ANNOTATION, so a callee
    /// declaring more params reads registers the caller never filled
    /// — these bindings dispatch through the boxed dual entry
    /// instead (argc + undefined-filled argv + per-face adapter
    /// unbox, the same lane a rest-typed binding rides).
    pub fnsig_mismatch_bindings: std::collections::HashSet<String>,
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
    /// `call ExprId → intact Member callee ExprId` for the
    /// `this`-receiver half of the same Pass 2 rewrite — the half
    /// `speculative_cm_rewrites` deliberately skips (a `this`
    /// receiver's type is always the enclosing class, so no demotion
    /// decision exists). The twin mint reads it to restore the
    /// member-call shape inside a cloned `any`-receiver body, where
    /// the static `__cm_` / `__dispatch_` call would reject at the
    /// checker's ClassRef arg gate (RFC 20260804 blade 2).
    pub cm_this_static_calls: std::collections::HashMap<ExprId, ExprId>,
    /// RFC 20260804-method-rebind-generic-body — mono class-method
    /// name (`__cm_<C>__<m>`) → its receiver-polymorphic twin
    /// (`__cmany_<C>__<m>`). Minted by `desugar_classes_generic_twin`
    /// for every instance method whose body reads `this`; consumed by
    /// the boxed-adapter synthesis (blade 3), which guards on the
    /// receiver's class tag and routes foreign receivers to the twin.
    pub cmany_twins: std::collections::HashMap<String, String>,
    /// RFC 20260804-fn-this-channel knife 3a — mono static-method name
    /// (`__sm_<C>__<m>`) → its receiver-polymorphic twin
    /// (`__smany_<C>__<m>`), recorded only when the mint actually
    /// landed (a `this`-free body and a super-carrying body drop it and
    /// keep mono behavior). Read by the inherited-static call rewrite,
    /// which needs to know the receiver channel exists BEFORE the
    /// `fn_sigs` table the `.call` devirtualization consults is built.
    pub smany_twins: std::collections::HashMap<String, String>,
    /// RFC 20260724-regex-literal-syntax-error — side-channel populated
    /// by `ast_desugar_regex_syntax_error::run`. Keyed by `Expr::Regex`
    /// ExprId; value = the SyntaxError message to raise when the
    /// literal is entered at runtime. Chunk 1 populates this table
    /// (pre-parse each `Expr::Regex` via `torajs-regex` parser; when
    /// parse fails, record a spec-shaped SyntaxError message) and
    /// rewrites `Stmt::LetDecl { init: Expr::Regex(malformed) }` at
    /// the top level into `Stmt::Throw(new SyntaxError(msg))`.
    /// Non-let-init positions (nested stmts, expr-position) stay
    /// silent-wrong pending chunk 2 (expr-position IIFE wrapper).
    pub regex_parse_errors: std::collections::HashMap<ExprId, String>,
    /// Block / switch-CaseBlock redeclaration early errors (§14.2.1 /
    /// §14.12.1), filled by `ast_early_redecl::run` on the raw
    /// post-parse AST. Same pipeline-consumer shape as
    /// `regex_parse_errors`: the CLI drivers print each entry with
    /// the `parse error:` prefix and exit 1 before any desugar runs.
    pub redecl_parse_errors: Vec<String>,
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
    /// wedge admits beyond-arity calls through these names (the
    /// count itself rides the S1 hidden argc since RFC
    /// 20260810-indirect-argc-abi S3.2/S3.4 — no injected slot, no
    /// call-site prepend remains).
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
    /// Rotation 204 — computed `['__proto__']` keys join (§B.3.1
    /// step 5 requires IsComputedPropertyKey = false for the proto
    /// set): the computed fold to a plain field name records its
    /// value expr through the same channel.
    pub objlit_shorthand_proto_exprs: std::collections::HashSet<ExprId>,
    /// S2.24 刀 4 — value ExprIds of CoverInitializedName fields
    /// (`{ x = D }`, §13.2.5.1): the parser reads the cover grammar
    /// as the field value `Expr::Assign(x, D)` — the exact shape the
    /// pattern walk's `f: y = D` default arm consumes — and records
    /// it here. A literal that survives to expression position must
    /// early-error on it (check_type_of_object_lit); only a
    /// destructuring re-read may consume it.
    pub objlit_cover_init_exprs: std::collections::HashSet<ExprId>,
    /// Array literals whose last element is a spread followed by a
    /// trailing comma (`[...x,]`). Legal as an ArrayLiteral
    /// expression (§13.2.4 permits the trailing comma), but the
    /// cover-grammar re-read as an assignment PATTERN requires the
    /// rest element to be last (§13.15.1) — the dstr-assign walk
    /// rejects members of this set. The comma is consumed at parse
    /// time; this side table is the only carrier of that fact.
    pub arrlit_trailing_comma_after_rest: std::collections::HashSet<ExprId>,
    /// S2.24 刀 4 (widened rotation 434) — Member ExprIds minted by
    /// `dstra_field_load` for EVERY pattern field (`{ f }` / `{ f = D }`
    /// / `{ f: y = D }`, both lanes — declaration and assignment share
    /// the recipe). §13.15.5.4 GetV answers `undefined` for an absent
    /// field on every slot — a default only changes what replaces that
    /// `undefined` — so these reads are LENIENT on a static miss: the
    /// checker answers `Any` instead of "no member" and the lowering
    /// rides the any-member runtime read — a static miss is not a
    /// runtime miss (prefix-widened heterogeneous array elements type
    /// by the anchor but may really carry the field). Only marked
    /// reads — a user member read keeps the hard error (TS posture).
    pub dstr_default_member_loads: std::collections::HashSet<ExprId>,
    /// rotation 233 — Member ExprIds minted by the parser's `await e`
    /// desugar (`e.value`, expr_prec L.2). §27.7.5.1 Await: a Promise
    /// unwraps to its settled value, ANY other operand passes through
    /// identity (`Promise.resolve(e)` of a non-thenable yields e) —
    /// the read is by TYPE, never a field lookup. Unmarked `.value`
    /// reads keep the field-lookup path, so a user's `{value: T}`
    /// struct member is untouched. Without the mark the checker had
    /// to guess from the name alone and let Struct receivers win the
    /// field, so `await {value: 1}` answered `1` (silent wrong).
    pub await_value_reads: std::collections::HashSet<ExprId>,
    /// RFC 20260725-objlit-computed-key 刀 1 — value ExprId → key
    /// ExprId of `{ [expr]: v }` / `{ [expr]() {} }` computed-key
    /// fields. The field name in the flattened `(String, ExprId)`
    /// shape is a unique `__computed_<n>__` sentinel; the real
    /// property name is the key expr's runtime ToPropertyKey result,
    /// evaluated by the dynobj-init lane in field order. Literal
    /// string keys still fold at parse time and `Symbol.`-chains
    /// keep their `__sym_<chain>__` encoding (the iterator-protocol
    /// consumers), so neither appears here.
    pub objlit_computed_keys: std::collections::HashMap<ExprId, ExprId>,
    /// P-SURF S2.27 — the accessor subset of `objlit_computed_keys`:
    /// `{ get [expr]() {} }` / `{ set [expr](v) {} }` faces, keyed by
    /// the face ExprId, `true` = getter. The dynobj-init computed arm
    /// routes a member in this set through the accessor define kernel
    /// (`DefineKey::Expr`) instead of the data-field store.
    pub objlit_computed_accessors: std::collections::HashMap<ExprId, bool>,
    /// RFC 20260802-class-computed-member 刀 2 — `(class, sentinel)`
    /// → key ExprId of a runtime computed class member name
    /// (`class C { [expr]() {} / get [expr]() {} }`). The member
    /// installs under a unique `__ccm_<n>__` sentinel through the
    /// ordinary method / accessor machinery; desugar_classes splices
    /// a `__torajs_class_computed_reify` call at the class-decl
    /// position (ClassDefinitionEvaluation order — the key expr must
    /// see bindings declared above the class), which ToPropertyKey's
    /// the key and defines the reified face onto `__proto_<C>` /
    /// `__class_<C>`. Literal string / numeric whole keys fold at
    /// parse time and `Symbol.`-chains keep `__sym_<chain>__` (刀 1),
    /// so neither appears here.
    pub class_computed_keys: std::collections::HashMap<(String, String), ExprId>,
    /// RFC 20260802 刀 3 后半 — STATIC runtime-computed class FIELDS:
    /// `(class name, `__ccm_<n>__` sentinel, init ExprId)` in
    /// declaration order. The key expr lives in
    /// [`Self::class_computed_keys`] under the same sentinel; pass 3
    /// emits `let __ccmk_<C>_<n> = <key>` plus a keyed store onto the
    /// class object at the class-decl position. INSTANCE computed
    /// fields ride `field_inits` under their sentinel name instead
    /// (the ctor-prefix injection rewrites them to keyed writes).
    pub class_computed_static_fields: Vec<(String, String, ExprId)>,
    /// RFC 20260814-capturing-nested-class blade 3 — why the
    /// capturing-nested-class lane turned a given class down,
    /// recorded WHERE THE DECISION IS MADE (the nested-class hoist),
    /// in declaration order.
    ///
    /// The checker is what finally reports it, and by then the tree
    /// has moved: a static method's `this` is gone from the body it
    /// was declined for, so recomputing the reason at that point
    /// answered a different one — or none, which printed the
    /// "the class desugar did not claim it" fallback at someone who
    /// wrote ordinary TypeScript. Keyed by class name, which is not
    /// unique; two declined classes sharing a name can trade reasons
    /// (a message, never a semantic).
    pub unclaimed_class_reasons: Vec<(String, &'static str)>,
    /// Bindings the capturing-class lane lowered — the ones a later
    /// sibling may `extends` (RFC 20260814 blade 5; 405-01 opened the
    /// static-carrying half through the function value's user
    /// [[Prototype]] chain, so the static-free restriction is gone).
    /// Keyed by the lane's minted `__cc<N>_<name>` binding name, which
    /// the α-rename has already written into a routed sibling's
    /// `parent` field by the time that sibling asks.
    pub es5_parent_classes: std::collections::HashSet<String>,
    /// Where a lane-claimed class's `super(…)` actually lands (405-01):
    /// each minted binding maps to the nearest ancestor — itself
    /// included — that declares an EXPLICIT constructor. A class with
    /// no ctor contributes nothing to construction beyond forwarding,
    /// and the synthesized forwarder (`function (...args) {
    /// P.call(this, ...args) }`) is a rest-param `this`-reader — the
    /// one shape the receiver-promotion ABI bar refuses when a
    /// subclass then consumes it through `.call`. Skipping the silent
    /// middle is transitively equivalent and keeps every hop off that
    /// bar.
    pub es5_ctor_forward: std::collections::HashMap<String, String>,
    /// Top-level REAL classes a capturing subclass may `extends`
    /// (405-01 face 2): root (no heritage), non-generic,
    /// non-abstract, and program-wide name-unique. Filled by the
    /// hoist's entry scan — root-ness is what guarantees the ctor
    /// body carries no super form, so the `__ctorany_` mint below
    /// can never drop, and the admit decided here stays honest.
    pub top_root_real_classes: std::collections::HashSet<String>,
    /// The real classes some capturing subclass actually `extends`
    /// (405-01 face 2) — the lane's admit records the parent here,
    /// and `desugar_classes` mints the receiver-polymorphic ctor
    /// twin `__ctorany_<C>` for exactly these (the lane's `super(…)`
    /// spelling calls it directly: a real class's `.call` correctly
    /// throws per §10.3.1, and the mono `__cm_<C>__ctor` reads its
    /// receiver at baked struct offsets a dynobj instance never
    /// has).
    pub es5_real_parents: std::collections::HashSet<String>,
    /// RFC 20260815 knife 2a — the minted `__ccp<N>` bindings holding
    /// an extracted non-Ident heritage expression (`class K extends
    /// f() {}` → `let __ccp0: any = f()`). The capturing lane admits a
    /// class whose parent names one of these: its ES5 machinery is
    /// already runtime-shaped, so the parent being a VALUE costs
    /// nothing new. `super(…)` rides the `P.call(this, …)` arm — right
    /// for a function-shaped parent value; a real-class value throws
    /// §10.3.1's TypeError loudly (the runtime ctor-twin dispatch is
    /// knife 2b). Filled by `extract_value_heritage`.
    pub es5_value_parents: std::collections::HashSet<String>,
    /// Rotation 410 — the subset of [`Self::es5_value_parents`] whose
    /// heritage was the literal `null` (§15.7: legal; the class
    /// object's own [[Prototype]] stays %Function.prototype%, its
    /// `prototype`'s is null, and `super(…)` / the implicit ctor
    /// throw at `new` time). The lane's proto link reads this to
    /// emit `Object.create(null)` instead of `Object.create(
    /// P.prototype)` and to skip the class-side `setPrototypeOf`.
    pub es5_null_parents: std::collections::HashSet<String>,
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
    /// The `"use strict"` string literals the parser WROTE rather than
    /// read — one per function body that is strict only by lexical
    /// inheritance (§11.2.2), materialized so the desugar chain can
    /// read strictness off the body itself (see
    /// `parser::strict_directive`). `tr fmt` skips exactly these: a
    /// synthetic directive re-emitted into a function with a
    /// non-simple parameter list would be the §15.1.3 SyntaxError.
    pub synth_strict_directives: std::collections::HashSet<ExprId>,
    /// Sites that are legal in sloppy script code and a SyntaxError
    /// under a strict goal, parked because the goal is only stamped
    /// after the parse. `(byte offset, word)` — the word names the
    /// clause, the three being disjoint: a §12.7.2 future reserved
    /// word (`static`, `public`, …) in any Identifier position,
    /// `eval` / `arguments` where §13.1.1 binds or §13.1.3 assigns
    /// them, and `with` per §14.11.1. The prelude gate
    /// `triage_strict_reserved_idents` raises the first one. The
    /// per-FUNCTION half never lands here — the parser rejects it on
    /// the spot, knowing its own `in_strict_fn`.
    pub strict_reserved_positions: Vec<(u32, String)>,
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
    /// bodies carry the synthetic `__torajs_argv` param (their argc
    /// rides the S1 hidden slot since S3.4) and a materialized
    /// `__torajs_arguments` local; the checker mints their Function
    /// type rest-tail (`(...args: any[]) => R`) so every call rides
    /// the boxed dual entry (which feeds real argc + argv).
    pub closure_argv_fns: std::collections::HashSet<String>,
    /// RFC 20260708-closure-argv-face — bindings holding a
    /// `closure_argv_fns` value (aliases included). Recorded for
    /// symmetry with `closure_argc_locals`; the SSA variadic
    /// registration rides the checker's rest-tail type instead.
    pub closure_argv_locals: std::collections::HashSet<String>,
    /// Rotation 365 — receiving-fn params whose direct calls must
    /// route through the boxed dual entry: `fn name → {param names}`.
    /// Filled by the argv tier's fn-arg track (an anonymous argv-face
    /// fn-expr passed directly as that param's argument): the body
    /// grew argc/argv slots, so the param's static-dispatch lane
    /// would feed the reshaped signature garbage — the boxed adapter
    /// is universal across closures, so routing the param's calls
    /// boxed is behavior-preserving for every other value that can
    /// flow in. Scoped by construction (per-fn key, param-name
    /// value); consumed by the SSA `setup_fn_params` variadic
    /// registration.
    pub argv_boxed_params: std::collections::HashMap<String, std::collections::HashSet<String>>,
    /// RFC 20260801-arguments-method-face knife 4a — `__cm_` method
    /// bodies admitted to the runtime argv face: reached only through
    /// member-value reads (zero arena Ident references), so every
    /// call rides the boxed adapter, which forwards the true argc
    /// into the S1 hidden sig slot (S1-T1/T2) and the argv pointer
    /// into the injected `__torajs_argv` param. The checker reads
    /// this to strip TWO leading params (`__this` + argv) when
    /// answering the method's value type, instead of the usual one.
    pub method_argv_fns: std::collections::HashSet<String>,
    /// RFC 20260810-indirect-argc-abi H1 — head-less top-level fns
    /// on the T-31 real-argc tier (`arguments.length` readers with
    /// no `__env`/`__this` head). Their SSA sig carries the hidden
    /// I64 `__torajs_argc` at position 0 (no head to sit behind);
    /// the direct-call terminal consults this set to shift its
    /// param-type reads past the slot and prepend the argc operand.
    /// Head-less bodies have no self-describing param marker (the
    /// injected `injected argc param` retires with the H blades), so
    /// the family needs this side table where the env/this tiers key
    /// on param names.
    pub headless_argc_fns: std::collections::HashSet<String>,
    /// RFC 20260816-headless-argv-face — the subset of
    /// [`Self::headless_argc_fns`] that also owns a runtime argv
    /// channel: a synthetic `__torajs_argv: __argvptr()` param at
    /// AST position 0, fed at every direct-call site from a stack
    /// buffer the terminal packs. Admission (module doc in
    /// `arguments_object_headless_argv`) requires every arena
    /// reference to the name be a call callee, so no site is missed.
    pub headless_argv_fns: std::collections::HashSet<String>,
    /// Head-less fns whose body reads argument VALUES at all —
    /// [`Self::headless_argv_fns`] plus the ones that wanted the
    /// channel and could not have it (a rest tail, a bare
    /// `return arguments[i]`, a value escape, a spread call site).
    /// A declined body keeps the declared-params approximation, so
    /// every lane that would widen its call sites must keep refusing
    /// it: the dynamic-spread forwarder wrap reads this set to tell a
    /// length-only head-less callee (safe to widen — the relay
    /// carries the true argc) from one whose `arguments[i]` would
    /// then answer undefined instead of failing loudly.
    pub headless_argv_touch_fns: std::collections::HashSet<String>,
    /// Call ExprId → the argument count the SOURCE wrote, for calls
    /// `apply_default_args` padded with the callee's declared
    /// defaults. The pad rewrites the arena node, so by lowering
    /// time the arg list no longer says how many arguments the
    /// program passed — and the head-less tier's hidden argc slot is
    /// filled from exactly that count, which made `a1()` on
    /// `function a1(x?: number)` answer `arguments.length === 1`.
    /// The face collectors are unaffected (they run before the pad).
    pub default_padded_argc: std::collections::HashMap<ExprId, usize>,
    /// Phase L.2 — names of `async function` declarations recorded by
    /// the parser. desugar_async iterates ast.stmts and, for any
    /// FnDecl whose name is in this set, wraps the return value in a
    /// Promise and shifts the surface return type from T to Promise<T>.
    /// Avoids adding an `is_async: bool` to every FnDecl construction
    /// site.
    pub async_fns: std::collections::HashSet<String>,
    /// §27.2.4 patch-consult bypass (rotation 448) — the
    /// `Promise.resolve(e)` / `Promise.reject(err)` Call exprs the
    /// async desugar synthesizes (return rewrites, the tail-safety
    /// return, the catch wrapper). Spec-wise those are the async
    /// machinery's INTERNAL settle operations (§27.7.5.2 works the
    /// promise capability directly), so a user patch on the Promise
    /// statics must never see them: the static call lowering routes
    /// members of this set straight to the typed fast lane, no patch
    /// probe emitted. Monomorph clones carry entries across the id
    /// map (`check_monomorph_clone_tables`).
    pub synth_promise_static_calls: std::collections::HashSet<ExprId>,
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
    /// Expression-position `yield*` done-value routing (§27.5.3.2:
    /// the YieldExpression's value is the inner iterator's final
    /// `.value`). The parser hoists `v = yield* src` into a mutable
    /// `__yx_<n>` temp plus the same yield-bearing ForOf the statement
    /// form emits; this maps that ForOf's element binding
    /// (`__ysxv_<n>`, unique per mint) to the temp so the F1 manual
    /// protocol's done arm writes `__step.value` into it before
    /// breaking. Keyed by name — clones share the entry safely, the
    /// binding scopes per generator body.
    pub yieldstar_done_targets: std::collections::HashMap<String, String>,
    /// Namespace-import bindings (`import * as ns` /
    /// `export * as ns`) materialized as synthetic object literals by
    /// the resolver. §10.4.6.8 — a module namespace exotic object
    /// answers `undefined` for a non-exported name, and namespaces
    /// are not extensible, so the checker's anonymous-struct
    /// typo reject does not apply to a member miss on one of these:
    /// it answers Undefined statically instead.
    pub namespace_bindings: std::collections::HashSet<String>,
    /// True when the parser saw at least one dynamic `import(...)`
    /// expression (any argument shape — §13.3.10 takes a full
    /// AssignmentExpression). Gates the module resolver's speculative
    /// candidate collection and `__torajs_dyn_import` dispatcher
    /// synthesis, so a program without dynamic import pays nothing.
    pub dyn_import_present: bool,
    /// Byte range of each class declaration (`class` keyword through
    /// the body's closing brace), keyed by class name. Feeds the
    /// §20.2.3.5 class-constructor toString source: the class-register
    /// lowering interns the type-erased slice and hands it to the
    /// runtime's per-tag source table. Nested parses (lib / eval /
    /// builtin injection) never keep entries here — `parse_into`
    /// drops what they add, since their spans index the wrong source
    /// text (the same mismatch `clear_injected_spans` guards) — so
    /// those classes answer the native form.
    pub class_decl_spans: std::collections::HashMap<String, crate::lexer::Span>,
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
    /// Hoisted generator locals whose declared type IS a generator
    /// class: field name → `__Gen_<name>`. A local living across a
    /// yield becomes a field of the enclosing `__Gen_*`, so a
    /// receiver spelled `this.__yieldstar_it_3` is a Member read by
    /// the time later passes see it, and the binding's annotation —
    /// the only place its class was written down — is gone with the
    /// `let`. Recorded here so `apply_default_args` can still resolve
    /// such a receiver precisely instead of falling back to its
    /// name-keyed method table. Field names are counter-minted and
    /// therefore unique program-wide.
    pub generator_local_classes: std::collections::HashMap<String, String>,
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
    /// RFC 20260810-sloppy-goal-arguments S1 — true when the input
    /// file compiles under the CommonJS sloppy-script goal, keyed on
    /// bun's file-extension goal mapping (`.ts` = ESM always-strict,
    /// `.cts` = CommonJS sloppy). The arguments-object desugar family
    /// reads it for the sloppy `callee` / mapped-aliasing faces;
    /// every other pass keeps the strict-goal behavior regardless.
    pub sloppy_script_goal: bool,
    /// RFC 20260814 — the parser met at least one §14.11 `with` and
    /// emitted its `{ let __with_<n> = …; body }` shape. `desugar_with`
    /// is the first prelude pass and would otherwise pay a whole-tree
    /// walk for every program; `with` is sloppy-goal-only and rare, so
    /// the flag buys the common case out of it entirely.
    pub has_with_stmt: bool,
    /// RFC 20260814 — the `Ident` occurrences `desugar_with` minted as
    /// the FALL-THROUGH arm of a guarded read (`has ? w.n : n`). That
    /// arm runs only when the object does not carry the name, and then
    /// the §14.11 answer is exactly what an unresolved read already
    /// does here: resolve lexically, else a runtime ReferenceError. So
    /// the read is correct as written and must NOT also print the
    /// `unknown identifier` diagnostic — the object usually does carry
    /// the name, and the warning would fire on every working program
    /// that says `with`.
    pub with_fallthrough_idents: std::collections::HashSet<ExprId>,
    /// RFC 20260814 — `Ident` nodes the PARSER wrote to name a global
    /// as machinery rather than because the program said the name:
    /// `Object.__forinKeys(src)` for a for-in head, `String(sub)` for a
    /// template substitution, `Promise.resolve(ns)` for a dynamic
    /// import.
    ///
    /// Every one of them spells a name a `with` object can carry, and
    /// `desugar_with` guarded them like any other free name — so
    /// `with ({ String: f }) { \`${v}\` }` called `f`, where §13.2.8.6
    /// runs the ToString abstract operation and never touches the
    /// `String` binding. Silently: a plausible wrong value with no
    /// diagnostic at all, which is the one outcome that pass is not
    /// allowed to produce. (The for-in shape failed louder but for the
    /// same reason — a guarded receiver is no longer the bare `Ident`
    /// its lowering recognises, so the call fell through to a dynamic
    /// member lookup of `__forinKeys`.)
    ///
    /// Keyed by NODE, not by spelling: the whole point is to tell the
    /// compiler's `Object` apart from the user's, and they are spelled
    /// the same.
    pub synth_global_refs: std::collections::HashSet<ExprId>,
    /// Byte positions where the parser admitted `yield` as an
    /// identifier (binding name or IdentifierReference) outside a
    /// generator body. §12.7.2 reserves the word under the STRICT
    /// goal only, and the goal bit above is stamped after parsing —
    /// so the parser admits and `ast::triage_yield_idents` (prelude)
    /// raises the strict-goal SyntaxError when this is non-empty.
    pub yield_ident_positions: Vec<u32>,
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

/// RFC 20260724-regex-literal-syntax-error — chunk 1 wrapper.
/// Rewrites top-level `let r = /malformed/` into
/// `throw new SyntaxError(...)`. See sibling module for scope.
pub fn desugar_regex_syntax_error(ast: &mut Ast) {
    crate::ast_desugar_regex_syntax_error::run(ast);
}

/// ES2025 `Promise.try` — rewrites `Promise.try(f, ...args)` into an
/// immediately-invoked try/catch arrow over the resolve/reject
/// statics. See sibling module for the shape and ordering contract.
pub fn desugar_promise_try(ast: &mut Ast) {
    crate::ast_desugar_promise_try::run(ast);
}

/// §8.6.2 default-parameter TDZ — rewrites a default that bare-reads
/// its own or a later parameter into a throwing IIFE (ReferenceError
/// at call time). See sibling module for scope and ordering.
pub fn desugar_dflt_param_tdz(ast: &mut Ast) {
    super::dflt_param_tdz::run(ast);
}

/// Block / switch-CaseBlock redeclaration early errors — must run on
/// the RAW post-parse AST (before generator / async / var-hoist
/// desugars move one side of a conflict away). See sibling module.
pub fn early_redecl_errors(ast: &mut Ast) {
    crate::ast_early_redecl::run(ast);
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
