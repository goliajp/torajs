//! The [`Ast`] arena type and its inherent methods — split out of the
//! parent under the 500-line file discipline (rotation 146), leaving
//! `ast.rs` as the module-tree and re-export face (the same shape
//! `torajs-core/src/lib.rs` settled into at chunk 486).

pub use super::named_prop_census::accessor_names::AccessorFieldNames;
use super::*;
pub mod optional_members;

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
    /// Class-field or declaration initializer → the field's name (`#x`
    /// for a private field) or the binder's, recorded by the parser
    /// before a desugar moves it into a slot whose name is tr's.
    /// §15.7.10 / §14.3.1.2: the initializer is a NamedEvaluation.
    pub field_init_names: std::collections::HashMap<ExprId, String>,
    /// Initializers of the `let`s a destructuring parameter desugars to.
    pub param_destr_inits: std::collections::HashSet<ExprId>,
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
    /// Lifted nested-function symbol (`__nested_<parent>_<name>_<n>`)
    /// → the name it was written with. §B.3.3's hoist renames the
    /// declaration so two blocks can each declare `f`, and the mint is
    /// not a name any source can spell; the fn-name registry reads
    /// this in its place so `.name` answers the source spelling, the
    /// same job `genexpr_names` does for a hoisted generator
    /// expression. Recovering it from the symbol is not possible —
    /// both halves of the mint may themselves contain `_`.
    pub nested_fn_names: std::collections::HashMap<String, String>,
    /// `new N()` sites where `N` names a `function` declared in an
    /// enclosing function scope, so the lexical answer is that
    /// declaration and not the class of the same name. `Expr::New`
    /// carries a NAME, and `desugar_classes` binds every one of them
    /// to the `__new_<C>` factory by a global lookup; these are the
    /// sites that lookup must not claim. Filled by
    /// `fn_shadowed_new::mark_fn_shadowed_new` before the class pass
    /// runs, because the rename that establishes the shadow happens
    /// long after it.
    pub fn_shadowed_new: std::collections::HashSet<ExprId>,
    /// Operands of a `delete` that are Super References — §13.5.1.2
    /// step 4.b makes each one a ReferenceError whatever the property
    /// is. The parser writes `super.x` as a member off the
    /// `__superbase__` marker, but the class / object-literal super
    /// desugars rewrite a delete operand to a plain member off a
    /// MINTED base (they must: routing it through the read kernel
    /// breaks `delete <a call>`), and the marker is gone by lowering.
    /// So the judgement is recorded where the fact is still known.
    pub super_delete_operands: std::collections::HashSet<ExprId>,
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
    /// The `__`-prefixed identifier spellings the PROGRAM TEXT
    /// contains, recorded from the LEXER's token stream — not the
    /// expression arena, which the parser itself mints into
    /// (`ast::record_source_dunder_idents`). The
    /// three undeclared-name checker sites carve `__` names out of the
    /// §6.2.5.5 runtime-ReferenceError posture as compiler-synthesized;
    /// `__` is tr's convention, not a reserved namespace, so this is
    /// what tells one of tr's own names from one the program spelled
    /// (sputnik writes `__ref` / `__key` / `__func` as user code).
    pub source_dunder_idents: std::collections::HashSet<String>,
    /// Function declarations spliced in from an eval text whose own
    /// code is SLOPPY (§19.2.1.1 — an indirect eval is sloppy global
    /// code whatever the calling program was; a direct one inherits).
    /// Annex B §B.3.3 applies to them even inside a module, and the
    /// span cannot say so: `desugar_eval::foreign_spans` blanks every
    /// spliced declaration's span to the `(0, 0)` sentinel because it
    /// indexes a different string, and every synthesized declaration
    /// carries that same sentinel. Name-keyed for the reason
    /// `Ast::async_fns` is.
    ///
    /// Recorded boundary: a name reaches this set from a parse that a
    /// later check may still refuse, and nothing takes it back out —
    /// the same orphan story `parse_eval_source` records for the
    /// expressions it leaves behind.
    pub sloppy_eval_fn_names: std::collections::HashSet<String>,
    /// The same splice from an eval text whose own code is STRICT: a
    /// block function there binds its block only (§B.3.3 does not
    /// exist for strict code), and the blanked span cannot say so.
    pub strict_eval_fn_names: std::collections::HashSet<String>,
    /// The mirror of the set above, for the other direction: names that
    /// an assignment inside STRICT eval text would create if it were
    /// sloppy. §19.2.1.1 gives eval code its own strictness, and a
    /// strict assignment to an unresolvable name throws rather than
    /// creating a global — but after the splice the statements are
    /// indistinguishable from the sloppy program around them, and the
    /// spans were blanked, so the name is what is left to carry it.
    /// Read by `sloppy_implicit_globals`, which folds it into the set
    /// of names it must not synthesize.
    pub strict_eval_assign_names: std::collections::HashSet<String>,
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
    /// Calls that were written as an optional chain ending in a call
    /// (`a?.m(args)`): the Call's ExprId → the chain's BASE.
    ///
    /// `desugar_optchain_calls` rewrites such a callee from `OptChain`
    /// to `Member`, so the checker and every dispatch lane read the
    /// ORDINARY member call of §13.3.9's hit branch. The chain may still
    /// short-circuit, so the result widens to `Any` at the checker and
    /// the lowering wraps the call in the base's nullish guard, with
    /// the arguments inside the hit block. The base is kept because the
    /// guard evaluates it once, and the callee reads it non-null.
    pub optchain_calls: std::collections::HashMap<ExprId, ExprId>,
    /// Optional chains written in parentheses: `(a?.m).call(t)` ends
    /// the chain at `)`, so the call past it does not short-circuit.
    pub paren_optchains: std::collections::HashSet<ExprId>,
    /// How many arguments a call site supplies before the tail, for
    /// every top-level declaration whose last parameter is a rest.
    /// Published by `apply_rest_args` out of the very reading it packs
    /// with, so the packer and the lanes that have no packing site
    /// cannot drift apart about where a tail begins.
    ///
    /// The receiver of a `__this`-first body is peeled: the lane that
    /// needs this table (sibling-class dispatch) resolves the receiver
    /// itself and never carries it in the argument list.
    pub rest_arg_prefix: std::collections::HashMap<String, usize>,
    /// Class name → index in the constructor body just past the
    /// parser-prepended field initializers (`this.x = init`, after
    /// the `super()` when there is one). §10.2.2 [[Construct]]
    /// initializes the fields BEFORE FunctionDeclarationInstantiation
    /// runs the parameter defaults, so the default-materialization
    /// guards for `__cm_<C>__ctor` splice in here, not at the head.
    pub ctor_field_init_end: std::collections::HashMap<String, usize>,
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
    /// §16.2.1.6 — reads the module resolver judged unresolvable in
    /// THEIR module: an `Ident` in one module's statements naming a
    /// top-level declaration only another module made. The flat top
    /// level the splice builds would resolve the name; a module's own
    /// environment record has no binding for it. The checker treats a
    /// marked read (or write target) as an unresolvable Reference
    /// before any lexical lookup. Filled by `modules::module_scope`.
    pub module_foreign_reads: std::collections::HashSet<ExprId>,
    /// RFC 20260810 — writes to a fn-expression's own self-name
    /// (§15.5.5: an immutable function-env binding; a strict-mode
    /// write — module code always is — raises TypeError at run time,
    /// not a compile reject). The checker marks the TARGET ident eid,
    /// the assign lane raises through the readonly-assign kernel.
    /// Parked by `check_monomorph` on the owned post-check AST (same
    /// ride as `undeclared_reads`). Empty pre-check.
    pub self_name_writes: std::collections::HashSet<ExprId>,
    /// Writes to a `const` / `using` / `await using` binding, or to an
    /// import binding — §6.2.5.6 PutValue reaches §9.1.1.1.5
    /// SetMutableBinding, and these are immutable **strict** bindings
    /// (CreateImmutableBinding(N, true) / CreateImportBinding), so
    /// step 5 forces S to true and the write raises TypeError at run
    /// time. The checker marks the TARGET ident eid and the assign
    /// lane raises through the same readonly-assign kernel
    /// [`self_name_writes`] uses.
    ///
    /// Kept apart from that set because the STRICTNESS differs, not
    /// the kernel: a fn-expression's self-name is created with
    /// S = false, so a sloppy function's write to it is a silent
    /// no-op, while a write to a const throws in sloppy code too.
    pub const_writes: std::collections::HashSet<ExprId>,
    /// Inits `desugar_uninit_let` SPLICED in — the binding was
    /// written `let x;` and the first `x = EXPR` was promoted into
    /// its init slot. After that rewrite the two spellings are
    /// indistinguishable in the arena, and tsc judges them
    /// differently: `var o; o = {j(){}}; o = {}` is an evolving
    /// `any` and accepted, while `var o = {j(){}}; o = {}` is
    /// TS2741. [`crate::let_widen`] is the reader.
    pub spliced_uninit_inits: std::collections::HashSet<ExprId>,
    /// Operations the CHECKER proved can only throw, with the message
    /// to throw. `docs/language-spec/ch1-type-runtime-boundary.md`
    /// calls this band B: the spec defines a runtime behaviour for the
    /// operation (§13.15.3 ToNumeric disagreeing, §13.6, §7.1.4) and
    /// both operand types are known at compile time, so the outcome is
    /// decided here rather than re-derived at run time.
    ///
    /// An entry means "evaluate the operands for their effects, then
    /// raise" — `ssa_lower_spec_throw` emits exactly that, with no type
    /// dispatch. Parked by `check_monomorph` on the owned post-check
    /// AST (same ride as `undeclared_reads`). Empty pre-check.
    pub spec_throws: std::collections::HashMap<ExprId, String>,
    /// Some object literal leaves out a member of its declared type, so
    /// the own-property folds ask the cell. Parked post-check.
    pub mints_absent_set: bool,
    /// Lifted closures whose return type was SEEDED from the position
    /// they were passed to (a builtin callback slot, a fn-typed
    /// parameter / binding), not written by the user. The annotation
    /// is the context's guess, so a value that breaks it is converted
    /// the way the spec converts that callback's result, where a
    /// user-written return annotation is a checked cast (RFC
    /// 20260922-any-to-typed-checked-cast, slice S1b). Filled by
    /// `apply_closure_ann_updates` at the one point it writes the seed.
    pub contextual_ret_fns: std::collections::HashSet<String>,
    /// Each function's source span (start, end) → whether the program
    /// wrote its return type, as opposed to the compiler inferring it.
    /// Filled at parse by `ast::ret_written`; read through
    /// [`Ast::ret_written_at`].
    pub ret_written: std::collections::HashMap<(u32, u32), bool>,
    /// Monomorph clones (no span of their own) of a function whose
    /// return type the program wrote, by clone name.
    pub ret_written_clones: std::collections::HashSet<String>,
    /// §8.1.1.1 — top-level `let` / `const` occurrences written
    /// EARLIER in the statement list than the declaration, so the
    /// binding is in its temporal dead zone whenever they run. Reads
    /// AND write targets, together for the same reason
    /// `undeclared_reads` keeps both: §6.2.5.5 GetValue and §6.2.5.6
    /// PutValue are the same Reference. Filled last in the AST
    /// pipeline by `ast::mark_tdz_reads` (which documents why
    /// this one shape is decidable and where the walk stops); the
    /// ident and assign lanes each raise a catchable ReferenceError at
    /// the occurrence. Empty pre-pipeline.
    pub tdz_reads: std::collections::HashSet<ExprId>,
    /// 551-03 — any-widened clone of a lifted closure body → the
    /// lifted name it was cloned from. Never constructed (the call
    /// retargets to it with the ORIGINAL cell's env), so its env
    /// layout is the original's. Parked by `check_monomorph`.
    pub widen_clone_origin: std::collections::HashMap<String, String>,
    /// Every any-widened clone `check_monomorph_any_widen` minted, and
    /// the lifted closures cloned along with one.
    pub any_widen_clones: std::collections::HashSet<String>,
    /// RFC 20260714-dstr-residual blade 4 — NamedEvaluation for
    /// anonymous class expressions: synth class name
    /// (`__ClassExpr_<id>`) → binding identifier, recorded by the
    /// parser at `let D = class {}` bindings and destructuring
    /// defaults. `synthesize_class_globals` uses it as the `.name`
    /// field of the class value global. A named class expression
    /// (`class Inner {}`) parses under its own name and never
    /// enters this map (§15.5.5: the self-name wins).
    pub class_expr_display_names: std::collections::HashMap<String, String>,
    /// A binding renamed out of a `globalThis.X` read's way: new
    /// spelling → the name the source wrote, for the messages that
    /// quote it (a dead-zone read's ReferenceError).
    pub renamed_binding_display: std::collections::HashMap<String, String>,
    /// Rotation 373 (L3b 373-05) — synth `__ClassExpr_<id>` decls
    /// whose expression site sat in an enclosing class body's field
    /// initializer or computed key (K6-1 lands one written in a
    /// member body's statement beside its use site instead). Their
    /// evaluation is deferred past the enclosing class's definition,
    /// so `class extends Outer` from within Outer's own body is
    /// legal — but the parser's synth flush lands the decl BEFORE
    /// the enclosing ClassDecl stmt, where the M5.2 source-order
    /// check would reject the parent as a forward reference.
    /// `compute_full_fields` resolves members of this set in a
    /// second, dependency-ordered pass instead.
    pub class_expr_deferred: std::collections::HashSet<String>,
    /// Rotation 672 (K6) — the immediately invoked arrows the parser
    /// wraps a nested class's static-initializer class expression in,
    /// keyed by the call, answering the class's synthesized name. Only
    /// these see §8.4 NamedEvaluation through the call; a user-written
    /// `(() => class {})()` does not.
    pub class_expr_iife_calls: std::collections::HashMap<ExprId, String>,
    /// RFC 20260917-class-eval-unit K1 — hoisted classes whose every
    /// evaluation mints its own class object (`ast::class_unit`).
    pub eval_unit_classes: std::collections::HashSet<String>,
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
    /// RFC 20260916-private-element-brand slices 2a / 2b — each class
    /// owning private elements on a covered chain → (the position of
    /// its first element in the chain's install order, how many of its
    /// elements install at its elements start) (`ast/private_brand.rs`).
    pub private_elements: std::collections::HashMap<String, (u32, u32)>,
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
    /// The checker's half of that census, keyed by the declaration (its
    /// initializer) so a same-named binding elsewhere keeps its direct
    /// calls: a fn-typed binding a store hands a value of another face
    /// than its annotation (`check_stmt_let_decl::fn_face_slot`).
    pub face_mismatch_lets: std::collections::HashSet<ExprId>,
    /// The top-level ones among them by name, for a call that reaches
    /// the binding as a module global.
    pub face_mismatch_globals: std::collections::HashSet<String>,
    /// The property and return slots a store hands a function value of
    /// another face, and the calls through any such slot typed `any`.
    pub face_slots: face_slots::FaceSlots,
    /// Expressions typed `Record<string, number>`: their keyed accesses take the dictionary lane.
    pub number_dicts: std::collections::HashSet<ExprId>,
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
    /// Static monos whose body reads `this` (the twin exists for the
    /// rebind); a static twin minted only for its boolean / integer
    /// slots is not here, and its face may run the mono this-free.
    pub smany_reads_this: std::collections::HashSet<String>,
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
    /// T-24 — virtual method index. Populated for every OVERRIDDEN
    /// method name (some owner is a strict descendant of another —
    /// `desugar_classes_method_owners::vtable_methods`, rotation 507):
    /// the single-chain names go through `__dispatch_<M>`, the rest
    /// through the sibling-dispatch lane's Member-shape sites. Each
    /// gets a stable u32 slot in the per-class vtable; the lowering
    /// loads `vtable_ptr[method_index] -> fn_ptr` and `CallIndirect`s.
    /// Indices are sorted by method name so codegen is reproducible.
    pub method_index: std::collections::HashMap<String, u32>,
    /// Interface witness slots, after the `method_index` ones
    /// (`crate::iface_witness`).
    pub iface_slots: crate::iface_witness::IfaceSlots,
    /// The members declared optional (`optional_members`).
    pub optional_members: optional_members::OptionalMembers,
    /// M-OO.5 — `(class_name, member_name)` → visibility, populated by
    /// the parser when a `private` / `protected` modifier appears on a
    /// field or method. Public is the absent-default (no entry stored).
    /// `static_fields` and `static_methods` get the same treatment —
    /// the entry's `class_name` is the class's own name regardless of
    /// instance-vs-static. check.rs reads this map at every Member
    /// access site to enforce the modifier.
    pub member_visibility: std::collections::HashMap<(String, PropKey), Visibility>,
    /// M-OO.5 — `(class_name, field_name)` set of `readonly` fields.
    /// Both instance and static fields can be readonly. check.rs rejects
    /// `obj.field = ...` (instance) and `Class.field = ...` (static)
    /// when the entry is present. Readonly inside the constructor /
    /// class init context is allowed; check.rs's caller-context tracking
    /// lifts the restriction for the same path that visibility uses.
    pub readonly_fields: std::collections::HashSet<(String, PropKey)>,
    /// RFC 20260915 #8 — machine class → the lifted locals that were
    /// `const` / `using` bindings. A lifted field is a writable slot,
    /// so the binding's immutability has to be re-stated here: the
    /// checker rejects every store to one of these fields except the
    /// lift's own initialization (`lifted_init_assigns`), with the
    /// sync lane's message. Keyed by class so a nested machine
    /// writing through its frame handle (`__genouter_N.<f> = …`) is
    /// caught too.
    pub const_lifted_fields: std::collections::HashMap<String, std::collections::HashSet<String>>,
    /// RFC 20260915 #8 — the `this.<f> = init` Assign nodes the lift
    /// minted, one per lifted declaration. Each is the binding's
    /// initialization, not an assignment, and is the only store a
    /// `const_lifted_fields` member admits.
    pub lifted_init_assigns: std::collections::HashSet<ExprId>,
    /// §15.7.1 — the root expression of every class field
    /// initializer, instance and static, named and computed, recorded
    /// where the early error is already checked.
    ///
    /// The ClassDecl a later pass sees no longer shows the instance
    /// ones: the parser folds them into the constructor prefix as
    /// `this.x = init`. §19.2.1.1 extends the same ContainsArguments
    /// prohibition to code a DIRECT EVAL brings into that position,
    /// and that check runs after the fold — so the position has to be
    /// written down while it is still visible.
    pub class_field_init_roots: Vec<ExprId>,
    /// P8.2 — accessor faces (ES §10.1.7): `(class, prop)` → the
    /// desugared `__cm_<class>__<prop>_get` / `_set` FnDecl, filled by
    /// `desugar_classes`. The checker reads them to type a Member read
    /// / an Assign-Member write; ssa_lower calls the face instead of a
    /// field Load / Store.
    pub accessor_getters: std::collections::HashMap<(String, PropKey), String>,
    pub accessor_setters: std::collections::HashMap<(String, PropKey), String>,
    /// Method-table names (`<key>_get` / `<key>_set`) whose owners are
    /// accessor faces, as opposed to a regular method spelled the same.
    pub accessor_slots: std::collections::HashSet<String>,
    /// RFC 20260718-accessor-reify 刀 3 — static-accessor twins of
    /// the instance maps above (`(ClassName, prop) → __sm_<C>__<p>_get
    /// / _set`). Reads/writes were already rewritten to face calls in
    /// desugar; class_globals reads these to emit the reify magic
    /// (AccessorPair own entry on the class object) and to keep the
    /// face FnDecls out of the static-METHOD reify sweep.
    pub static_accessor_getters: std::collections::HashMap<(String, PropKey), String>,
    pub static_accessor_setters: std::collections::HashMap<(String, PropKey), String>,
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
    /// The second and later `__proto__: v` values of one object
    /// literal (§B.3.1): an early error once the literal reaches
    /// expression position, legal in a destructuring re-read.
    pub objlit_dup_proto_exprs: std::collections::HashSet<ExprId>,
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
    /// rotation 610 — ExprIds whose property access was SPELLED with
    /// brackets and a literal key (`m["a"]`, `o[0]`), read or write. The parser
    /// folds `o["k"]` to a Member when the key is an identifier name
    /// (§13.3.2: `o["k"]` IS `o.k`, and the fold is what lets a
    /// literal key reach the class IC, accessors and static field
    /// layout), so by the time the checker sees it the two spellings
    /// are one node — and a Map, a Set, a RegExp or a String got the
    /// member checker's nominal reject for a name its hand-written
    /// builtin table happens not to list, while the SAME key spelled
    /// dynamically answered undefined off the runtime probe. One key,
    /// two answers, decided by whether it was a literal.
    ///
    /// So the fold records what it erased. A marked READ is LENIENT
    /// on a static miss (`answer_terminal_miss` → `Any`, and the
    /// lowering already rides the any-member probe for every one of
    /// these receivers): §10.1.8.1 [[Get]] answers `undefined` for an
    /// absent property, and a hand-written builtin surface is not the
    /// list of properties a value HAS. A marked WRITE on an OBJECT
    /// receiver lands in that object's own bag (§10.1.9.2
    /// OrdinarySet, which the member-store ladder's catch-all arm has
    /// implemented for every one of these shapes since rotation 527's
    /// write-side twin) — but never on a PRIMITIVE one, where the
    /// write is a strict-mode TypeError and answering it with a
    /// silent success would be strictly worse than the reject.
    ///
    /// An unmarked `o.k` keeps the hard error either way — that is
    /// the TS posture, and it is also how a missing builtin still
    /// surfaces loudly instead of silently becoming `undefined`.
    pub index_spelled_props: std::collections::HashSet<ExprId>,
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
    /// RFC 20260921 — counter behind [`Self::mint_class_decl_id`].
    pub next_class_decl_id: u32,
    /// RFC 20260921 — `__ccm_<n>__` sentinel → the `decl_id`s that
    /// declare it; see `ast::class_computed_owner` for why the class
    /// name cannot answer this and who may share a sentinel.
    pub class_computed_owner: std::collections::HashMap<String, Vec<u32>>,
    /// RFC 20260802 刀 3 后半 — STATIC runtime-computed class FIELDS:
    /// `(class name, `__ccm_<n>__` sentinel, init ExprId)` in
    /// declaration order. The key expr lives in
    /// [`Self::class_computed_keys`] under the same sentinel; pass 3
    /// emits `let __ccmk_<C>_<n> = <key>` plus a keyed store onto the
    /// class object at the class-decl position. INSTANCE computed
    /// fields ride `field_inits` under their sentinel name instead
    /// (the ctor-prefix injection rewrites them to keyed writes).
    pub class_computed_static_fields: Vec<(String, String, ExprId)>,
    /// 628-05 — INSTANCE runtime-computed class fields in declaration
    /// order: `(class, `__ccm_<n>__` sentinel, the own named field
    /// declared just before it — None when it leads its class)`. The
    /// layout has no row for such a field (its value lands in the +24
    /// expando dict), so this is the only record of WHERE among the
    /// named fields it was declared; pass 3 hands that anchor to the
    /// runtime (`__torajs_class_computed_field_register`), which is
    /// what lets `Object.keys` answer §10.1.11.1 declaration order.
    pub class_computed_instance_fields: Vec<(String, String, Option<PropKey>)>,
    /// 628-05 — a STATIC computed field's position in its class's
    /// `static_init` sequence (the index the next `StaticInit` step
    /// took when the field was parsed), keyed like
    /// [`Self::class_computed_keys`]. `emit_static_inits` runs the
    /// keyed store at that position among the named static fields —
    /// §15.7.14 defines the static elements in one ordered pass, and
    /// the class-decl-position patch ran the store ahead of every
    /// named static field declared above it.
    pub class_computed_static_field_pos: std::collections::HashMap<(String, String), usize>,
    /// §15.7.14 — the arena ids the parser allocated while parsing
    /// each static element, `(class name, static_init index, lo, hi)`
    /// for `lo..hi`. A static name that is BOTH a method and a field
    /// is the method inside the static initializers and blocks up to
    /// and including the field's own (every method is defined before
    /// any static initializer runs; the field's DefineField replaces
    /// it only once its initializer has been evaluated), and the
    /// field everywhere else — `desugar_classes_statics::
    /// positional_static_method_sites` tells the two apart by these
    /// ranges. Source ids only: a node minted by a later pass never
    /// lands in one.
    pub class_static_init_expr_ranges: Vec<(String, usize, u32, u32)>,
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
    /// The constructors a capturing subclass's `super(…)` may land on
    /// that are themselves DERIVED (carry an `extends` clause): the
    /// hoist's entry scan records every real-class candidate with a
    /// named parent, the lane's admit its own siblings. A super site
    /// is where the PARENT's §10.2.2 step 13 runs (`ctor_return.rs`),
    /// and step 13.c — a non-object, non-undefined answer is a
    /// TypeError — is gated on the parent being derived. Read before
    /// `class_parents` exists (the hoist runs first), which is why it
    /// is not derived from that table.
    pub es5_derived_parents: std::collections::HashSet<String>,
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
    /// RFC 20260920-struct-user-proto K3 — every class the compiler
    /// injects to stand in for a spec intrinsic (the error family,
    /// `DisposableStack` / `AsyncDisposableStack`). §10.1.13's default
    /// prototype is the nearest such class on a constructor's chain,
    /// `%Object.prototype%` when there is none; the runtime cannot tell
    /// an injected class from a user one by shape.
    pub intrinsic_classes: std::collections::HashSet<String>,
    /// K4 of the same RFC — the user's code names a re-parenting write
    /// (`ast::census_program_reparents`); typed class dispatch then
    /// tests the instance's `FLAG_OBJ_USER_PROTO` before going static.
    pub program_reparents: bool,
    /// User code can put a code-bearing named property on an array's or a
    /// string's prototype chain (`ast::named_prop_census`).
    pub array_prop_reads_may_raise: bool,
    /// The declared fields user code can turn into accessors
    /// (`ast::AccessorFieldNames`); a typed read of one tests the
    /// instance's `FLAG_OBJ_EXOTIC_FIELD` before loading the slot.
    pub accessor_field_names: AccessorFieldNames,
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
    /// Object-literal accessor slots whose closure is on the argv face,
    /// filled right after the arguments pass decides the faces.
    pub objlit_argv_slots: super::objlit_nominal_settle::ObjlitArgvSlots,
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
    /// String literals whose cooked value is `"use strict"` but whose
    /// source spells it with an escape or a line continuation. §11.2.1:
    /// such a literal still belongs to a directive prologue, but it is
    /// not a Use Strict Directive, so every strictness probe that reads
    /// a prologue's cooked values must leave these out.
    pub escaped_use_strict: std::collections::HashSet<ExprId>,
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
    /// Annex B legacy-octal sites the program spelled — `(byte offset,
    /// what it was)`. Same parking reason as the field above: sloppy
    /// script code evaluates them (the lexer gave every one its value),
    /// a strict goal rejects them, and the goal is only stamped after
    /// the parse. `ast::triage_legacy_octal` raises the first one.
    pub legacy_octal_positions: Vec<(u32, &'static str)>,
    /// Annex B function-declaration sites — `(byte offset, the
    /// position that admitted it)`. §B.3.2 and §B.3.4 hand a
    /// FunctionDeclaration back to a LabelledItem and to an
    /// IfStatement branch, and both extensions apply to sloppy code
    /// only. The parser refuses the ones it can already see are
    /// strict (`Parser::strict_here`); the goal half parks here, for
    /// the same reason as the two fields around it, and
    /// `ast::triage_annexb_fn_decls` raises the first one.
    pub annexb_fn_positions: Vec<(u32, &'static str)>,
    /// Duplicate-parameter sites — `(byte offset, the repeated name)`.
    /// §15.1.2 makes them a SyntaxError only where the
    /// FormalParameters are strict mode code; the parser refuses the
    /// sources it can see and parks the rest here, since under a
    /// strict goal every parameter list in the file qualifies.
    /// `ast::triage_duplicate_params` raises the first one.
    pub dup_param_positions: Vec<(u32, String)>,
    /// Annex B §B.3.5 for-in heads that carried an initializer —
    /// `for (var x = 5 in o)`. Byte offsets. The production is handed
    /// back to non-strict code only, so it parks for the same reason
    /// as the three fields above: `Parser::strict_here` refuses the
    /// halves the parser can see, and a module makes the rest of them
    /// SyntaxErrors too. `ast::triage_annexb_forin_init` raises the
    /// first one.
    pub annexb_forin_init_positions: Vec<u32>,
    /// Did THIS file's own directive prologue say `"use strict"`
    /// (§11.2.2, program level)? A bool rather than a read of
    /// `stmts[0]`, because by the time anything downstream asks,
    /// `modules::resolve_imports` has merged other files into that
    /// vector and the leading run of directives is no longer
    /// recoverable from it — the same reason
    /// `Parser::arm_strict_directive_program` takes the prologue state
    /// as a parameter instead of scanning. Recorded there and restored
    /// across nested parses by `parser::parse_into_eval`, so an
    /// imported module's prologue (or an eval text's) never answers
    /// for the entry. Read by `desugar_eval::walk::caller_strict_exprs`
    /// as one of §11.2.2's three sources.
    pub program_strict_prologue: bool,
    /// Byte ranges that are strict code for a reason the GOAL does not
    /// carry (§11.2.2) — read by the gates that can only judge after
    /// the parse, for which these are the regions where a sloppy-goal
    /// program still refuses the Annex B legacy-octal spellings above
    /// and the §B.3.3 function-in-block hoisting.
    ///
    /// Two of §11.2.2's three sources land here, because both are
    /// answerable as a byte range: an explicit `"use strict"` directive
    /// prologue (a function body, or the whole source at the program
    /// level — `parser::strict_directive`) and a class, every part of
    /// which is strict code including its heritage
    /// (`parser::parse_class_decl`). The third source is the goal
    /// itself, which is not a range and is asked separately.
    pub strict_code_spans: Vec<(u32, u32)>,
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
    /// Function-expression inits of function-local bindings used only
    /// as `.bind` receivers, given `__this: any` before the lift; the
    /// lifted closure in each slot joins `fnexpr_recv_fns` after it.
    pub fnexpr_bind_local_inits: std::collections::HashSet<ExprId>,
    /// Top-level `function` declarations `bind_this_param` promoted —
    /// their body had `__this` free, so it inserted the hidden
    /// receiver parameter and seeded `undefined` at every DIRECT call
    /// site. Recorded rather than re-derived by name-matching on
    /// `__cm_` / `__new_` prefixes, which `this_param.rs` deliberately
    /// avoids: a class member is bound by its parameter list and never
    /// qualifies, so the promoted set is the exact answer and a prefix
    /// test is only an approximation of it. Consumer:
    /// `namedfn_this_value::promote_this_fn_values`, which gives the
    /// same declarations' VALUE uses the matching forwarder.
    pub this_param_fns: std::collections::HashSet<String>,
    /// RFC 20260917-plain-fn-new-target — the top-level functions that
    /// gained a `(__this, __new_target, …)` head because they read
    /// `new.target` (`ast/new_target_param.rs`).
    pub new_target_fns: std::collections::HashSet<String>,
    /// Knife 2 — the function expressions (by ExprId, which the lift
    /// keeps) whose reads became a body-level `__new_target` local.
    pub new_target_fn_exprs: std::collections::HashSet<ExprId>,
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
    /// Class-member fns (`__cm_` / `__sm_`, constructors included)
    /// whose body touches `arguments`, recorded BEFORE
    /// `desugar_arguments_object` rewrites those idents away.
    ///
    /// `apply_spread_args` runs after that rewrite, so it cannot ask
    /// the question itself — it asks this record instead, the same
    /// way it already asks the head-less and closure faces. Without
    /// it a `super(...xs)` against an `arguments`-reading parent
    /// index-expanded to the ctor's declared arity and the real
    /// count died with the trimmed tail (`arguments.length` answered
    /// 0 while bun answered 3 — silent, rotation 616).
    ///
    /// Membership does not promise the fn has a runtime argv
    /// channel; it only says the static index-read expansion would
    /// lie. Sites kept off that expansion land on the runtime spread
    /// lane, which either carries the true count or rejects loudly.
    pub method_arguments_touch_fns: std::collections::HashSet<String>,
    /// K2b (RFC 20260917-class-eval-unit) — the `__cm_` / `__sm_` /
    /// `__new_` bodies that reach a PRIVATE element, by function name.
    /// Baked into each method's `MethodMeta` flags (bit 3) so the
    /// call-boundary guard can answer it from the face alone: two
    /// evaluations of one unit class share a layout, so running one's
    /// body against the other's instance is right for public members
    /// and a §7.3.31 brand failure for private ones.
    ///
    /// Collected right after `desugar_classes` mints those FnDecls and
    /// BEFORE `lift_arrow_fns` — after the lift, `m() { return (() =>
    /// this.#x)() }` keeps its private access in a `__closure_N` the
    /// method body no longer holds.
    pub private_touch_fns: std::collections::HashSet<String>,
    /// The this-first bodies admitted to the direct-call runtime argv
    /// face (module doc in `arguments_object_this_first_argv`), each
    /// with its head width: 2 for a constructor (`__this`,
    /// `__new_target`), 1 for a static twin or a promoted `this`-
    /// reading declaration. `__torajs_argv` sits right after the
    /// head, the SSA side gives the body the hidden argc slot the
    /// `__cm_` method tier gets, and the direct-call terminal packs
    /// both slots at that position.
    pub this_first_argv_fns: std::collections::HashMap<String, usize>,
    /// The this-first bodies on the direct-call runtime ARGC face,
    /// each with its head width — [`Self::this_first_argv_fns`] plus
    /// the length-only bodies that want the count and nothing else
    /// (rotation 650, the split the head-less tier has had since
    /// H1). An argc-only body gets the hidden I64 slot right after
    /// its head and no `__torajs_argv`; its direct-call sites insert
    /// the count alone. The split is what lets a relay's forwarding
    /// call pass its true runtime count through uncapped: an argv
    /// body caps the count at what the relay packed (its declared
    /// slots — the body would read past the buffer otherwise), so a
    /// typed HOF site reaching a length-only body through
    /// `__fwdrecv_` answered the declared arity (`[7].map(t, o)`
    /// said 1 where bun says 3).
    pub this_first_argc_fns: std::collections::HashMap<String, usize>,
    /// `__cm_` method → the argument count its body answers for
    /// (static-face count or declared arity), when its `__cmany_` twin
    /// reads the real argc / argv: the boxed adapter hands any call
    /// with another count to the twin.
    pub cm_fixed_argc: std::collections::HashMap<String, usize>,
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
    /// Which array callback loops may see their typed source shrink
    /// while they run (`hof_len_shrink` has the whole question).
    pub hof_len_shrinks: super::HofLenShrinks,
    /// Head-less fns whose body reads argument VALUES at all —
    /// [`Self::headless_argv_fns`] plus the ones that wanted the
    /// channel and could not have it (a rest tail, a bare
    /// `return arguments[i]`, a value escape that is not a call
    /// callee). A declined body keeps the declared-params
    /// approximation, so every lane that would widen its call sites
    /// must keep refusing it: the dynamic-spread forwarder wrap reads
    /// this set to tell a length-only head-less callee (safe to widen
    /// — the relay carries the true argc) from one whose
    /// `arguments[i]` would then answer undefined instead of failing
    /// loudly. An ADMITTED one wraps (its relay's adapter carries the
    /// values, rotation 625).
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
    /// Call ExprId → how many FIXED parameters the source filled,
    /// for calls `apply_rest_args` packed. The pack rewrites the
    /// arena node to `f(fixed…, [tail])`, so by lowering time the
    /// arg list is one slot wide past the prefix whatever the source
    /// wrote; the true count is this number plus the packed array's
    /// runtime length (a spread tail has no static length at all).
    /// `gr(1, 2)` on `function gr(...xs) { arguments.length }`
    /// answered 1 — the packed list's width.
    pub rest_packed_sites: std::collections::HashMap<ExprId, usize>,
    /// The `src[j]` reads `apply_spread_args` mints when it expands a
    /// trailing spread against a fixed-arity callee. They occupy an
    /// argument position but are not expressions the program wrote:
    /// the value comes off the spread source at a position the source
    /// may not reach, and §10.4.2.1 answers `undefined` there — which
    /// §10.2.11 then says the parameter simply IS. A boundary the
    /// program did not write is not one the checked-cast decision is
    /// about (RFC 20260922-any-to-typed-checked-cast C1); casting it
    /// turns `strs(...oneElement)` into a TypeError.
    pub spread_filled_args: std::collections::HashSet<ExprId>,
    /// The `undefined` idents `apply_default_args` mints for an
    /// ARITY HOLE — a parameter with no default sitting in front of
    /// one that has it, so the pad has to write something to reach
    /// the default behind it. §10.2.11 binds undefined at that slot,
    /// so that IS the value; what the program did not do is pass an
    /// argument, and the checker already has an answer for what an
    /// absent argument pins a type parameter to — the T-28 lane binds
    /// `Any` (`check_type_of_call_generic_ident::arg::unification_ty`
    /// carries it to the padded calls, which is the only reader).
    /// Padded DEFAULTS are not in here: `0` is a value the program
    /// wrote and the parameter is typed from it. Monomorph clones
    /// carry entries across the id map
    /// (`check_monomorph_clone_tables`).
    ///
    /// The mark deliberately does NOT reach the hole's expression
    /// type: it stays `Undefined`, because that is what the call-arg
    /// conversion reads to box ANY_UNDEF into a wide slot. Answering
    /// `Any` there instead handed the callee the bare ident's raw
    /// null and `m()` on `m(x, y = 5)` printed `null`.
    pub arity_hole_args: std::collections::HashSet<ExprId>,
    /// The nested `ObjectLit`s `default_init_for_field` mints when a
    /// class field's declared type is itself a class or an alias
    /// with a struct shape — value → the type name it stands for.
    ///
    /// Those literals are lowered INSIDE the owning class's factory
    /// (`__new_<C>`'s `__this` seed), and every per-cell identity the
    /// cell-construction tail writes — class tag, vtable pointer, the
    /// Error-derived flag — is taken from the enclosing factory's
    /// name. That reads "which factory am I lowering in", not "which
    /// class is this cell", and the two disagree for exactly these
    /// literals: `__new___Gen_gen` stamped its own tag on the
    /// three-field `__Gen_rf` cell it built for `__forof_it_0`, so
    /// the cycle collector — the only reader of `child_offsets` —
    /// walked a six-field layout off the end of a 56-byte
    /// allocation.
    ///
    /// A cell missing from this table keeps the factory-name
    /// behaviour, so losing an entry degrades to what the tail did
    /// before the table existed rather than mis-identifying anything
    /// new. Monomorph clones carry entries across the id map
    /// (`check_monomorph_clone_tables`).
    pub field_default_objlits: std::collections::HashMap<ExprId, String>,
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
    /// RFC 20260805-async-fn-state-machine blade 1 — async functions
    /// whose body now lives in a `__async_gen__<name>` generator driven
    /// by `__async_drive`. They STAY in `async_fns`: every "is this an
    /// async function" reader keeps its answer (the fnexpr-this receiver
    /// rule, any-widening, the async flavour bit on the fn value, the
    /// §15.8.4 param-throw posture). Only `desugar_async` consults this
    /// set, to leave the wrapper's `return __async_drive(...)` alone —
    /// the driver's `resolve` settles the result, a second
    /// `Promise.resolve` wrap would nest it.
    pub async_sm_fns: std::collections::HashSet<String>,
    /// Blade 2 — the async function VALUES (`async_fn_value_exprs`
    /// members) whose body now lives in a hoisted generator; the mark
    /// stays, this set only tells `desugar_async`'s value lane to leave
    /// the wrapper's `return __async_drive(...)` alone.
    pub async_sm_value_exprs: std::collections::HashSet<ExprId>,
    /// The promise expressions the async rewrite put under a
    /// `YieldInto` (`let v = await e` → `let v = yield e`): the
    /// generator yields `e` to the driver, which awaits it and sends
    /// the settled value back through `this.__sent`. The expansion
    /// reads this set to tell such a yield from a user-written one.
    pub await_yield_values: std::collections::HashSet<ExprId>,
    /// The `this.__sent` init the expansion minted for one of those →
    /// the promise expression it carries the settled value of. The
    /// width analysis reads the init as `e.value` — the promise's
    /// value point — so the binding stays in one width class with what
    /// resolved it, exactly as `let v = await e` does un-moved.
    pub yield_into_await_origins: std::collections::HashMap<ExprId, ExprId>,
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
    /// The loop variables of the `for await` an ASYNC generator's
    /// `yield*` was spelled as — the generator prep builds those loops
    /// as the delegation protocol (`desugar_generators_yieldstar`),
    /// not the for-await one.
    pub yieldstar_async_vars: std::collections::HashSet<String>,
    /// Namespace-import bindings (`import * as ns` /
    /// `export * as ns`) materialized as synthetic object literals by
    /// the resolver. §10.4.6.8 — a module namespace exotic object
    /// answers `undefined` for a non-exported name, and namespaces
    /// are not extensible, so the checker's anonymous-struct
    /// typo reject does not apply to a member miss on one of these:
    /// it answers Undefined statically instead.
    ///
    /// The value is the namespace's export list in resolver order:
    /// `(importer-visible export spelling, injected top-level local
    /// binding)`, the two differing when the deconflict pass mangled
    /// a colliding decl. `desugar_module_ns_members` reads it to
    /// retarget a static `ns.<export>` read straight at the local,
    /// leaving the object itself to the value uses §10.4.6 is about.
    pub namespace_bindings: std::collections::HashMap<String, Vec<(String, String)>>,
    /// Every namespace object literal's field values (`Ident`s of landed
    /// exports); one over a mutable export lowers to a cell over its slot.
    pub ns_binding_fields: std::collections::HashSet<ExprId>,
    /// Classes a type-only import named that no value walk landed:
    /// their declarations are in the program for their shape alone
    /// (TS `import type`), so a value use — `new C()`, `C` read as a
    /// value, `x instanceof C` — is the TS1361 compile-time refusal.
    pub type_only_classes: std::collections::HashSet<String>,
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
    /// RFC 20260921 — the same spans keyed by `decl_id`. Two classes
    /// may share a NAME, and the second parsed overwrote the first, so
    /// `String(C)` answered a sibling's declaration text. The by-name
    /// table above is rebuilt from this one once the desugar's renames
    /// have made the names unique.
    pub class_decl_spans_by_decl: std::collections::HashMap<u32, crate::lexer::Span>,
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
    /// RFC 20260912 — generator expressions whose body captures an
    /// enclosing local by reference, keyed by the hoisted factory name
    /// (`__genexpr_N`). Written by `hoist_gen_fn_exprs_wrapper`, read
    /// once by `desugar_generators` to fill `gen_env_methods`.
    pub gen_env_captures: std::collections::HashMap<String, GenEnvInfo>,
    /// The `__cm___Gen_*__{next,return,throw}` methods of those
    /// generators, keyed by method fn name: the checker seeds their
    /// scope and the lowerer their env preamble from the wrapper's
    /// capture layout (`GenEnvInfo::wrapper_fn_name`).
    pub gen_env_methods: std::collections::HashMap<String, GenEnvInfo>,
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
    /// Arrows whose rest param has no annotation (`(...xs) => …`).
    /// The parser gives it `any[]`, the type it has with no context;
    /// `lift_arrow_fns` carries the mark onto the lifted name as
    /// `implicit_rest_fns`, and `infer_anonymous_closure_params`
    /// replaces the `any[]` with the element type a variadic target
    /// declares (`(...xs: number[]) => R` types `xs` as `number[]`).
    pub implicit_rest_arrows: std::collections::HashSet<ExprId>,
    pub implicit_rest_fns: std::collections::HashSet<String>,
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
    /// The `__Gen_<name>` machine classes the generator desugar minted
    /// (a user class may carry the same spelling; this set may not).
    pub generator_machine_classes: std::collections::HashSet<String>,
    /// Generator decls whose parameter list contained binding patterns:
    /// fn name → count of parser-synthesized destructuring `let`s
    /// prefixed to the body, which desugar_generators moves into the
    /// `__Gen_<name>` ctor (eager binding at the factory call, §9.2).
    pub gen_param_destr_prefix: std::collections::HashMap<String, usize>,
    /// The same counts keyed by (name, span start, end): a nested
    /// generator's name is not unique, and a pass renaming one reads it.
    pub gen_param_destr_prefix_at: std::collections::HashMap<(String, u32, u32), usize>,
    /// Generator factories that need an all-`any` twin
    /// (`desugar_generators_any_twin`) no syntactic read reveals: a
    /// direct call handed an `any` argument, known only once checked.
    /// The checker records them on its mono AST; the driver then
    /// compiles the program again with this set seeded.
    pub generator_twin_requests: std::collections::HashSet<String>,
    /// A type declaration's fields as written, literal types kept
    /// (`check_type_ann::literal`), keyed by its name — the checker's
    /// alone; its AST fields spell each literal as its base type. `None`
    /// when no literal was written or more than one declaration has the
    /// name.
    pub literal_type_decls:
        std::collections::HashMap<String, Option<Vec<(crate::ast::PropKey, String)>>>,
    /// Plan A — Array literal ExprIds that the escape verifier proved
    /// safe to emit on the stack instead of `__torajs_arr_alloc_pooled`.
    /// Populated by `escape_analyze_array_literals`. ssa_lower's
    /// `Expr::Array` arm checks membership and switches between the
    /// AllocaBytes path (stack) and the heap path. Empty before the
    /// pass runs and for programs with no qualifying literals.
    pub stack_array_literals: std::collections::HashSet<ExprId>,
    /// RFC 20260821 attack B — the `.match` / `.exec` **callee**
    /// ExprIds whose result provably has no reader for the §22.2.7.8
    /// exec-shape properties (`index` / `input` / `groups`). Populated
    /// by `analyze_regex_result_props`; the two regex lowering
    /// dispatchers check membership and pass `want_exec = 0`, which
    /// makes the kernel skip building the arrprops side table
    /// (measured 16.6-18.5 ns per match). Empty before the pass runs
    /// and for every program that reads those properties anywhere.
    pub regex_result_props_unread: std::collections::HashSet<ExprId>,
    /// Rotation 468 — the init ExprIds of the `let` / `const` / `var`
    /// bindings that must hold OWNED elements: something in their
    /// scope writes an owned value INTO them (`X.push(v)` / `unshift`
    /// / `splice` / `fill` / `copyWithin` / `X[i] = v`) or the whole
    /// array escapes as a bare value (a call argument, another
    /// binding's init, an assignment value, a literal element, a
    /// return). Populated by `analyze_let_owned_elems`; the binding
    /// lane materializes a split product (an `Arr<Substr>` of views)
    /// into owned strings for the ones listed here and types the
    /// binding `Arr<Str>`. Empty before the pass runs.
    pub let_owned_elem_inits: std::collections::HashSet<ExprId>,
    /// The init ExprIds of the bindings whose alias class holds a
    /// split product as VIEWS in every member — bindings that only
    /// ever rebind or re-bind each other (`last = parts`, `let b =
    /// a`) with one of them a split call — each mapped to its whole
    /// class, itself included. The binding lane gives the `[]` and
    /// alias inits among them the `Arr<Substr>` layout at declaration
    /// once the checker's types confirm every split in the class
    /// answers views (`ssa_lower_stmt_let_decl_view`). Populated by
    /// `analyze_let_owned_elems`; disjoint from `let_owned_elem_inits`.
    pub view_slot_inits: std::collections::HashMap<ExprId, Vec<ExprId>>,
    /// The ExprIds whose value is consumed where it is produced — a
    /// binding's init, a member or index read's object, a discarded
    /// statement value. A split product keeps its views only in one
    /// of these; anywhere else it is handed on as owned strings.
    /// Populated by `analyze_let_owned_elems`.
    pub view_home_exprs: std::collections::HashSet<ExprId>,
    /// The separator ExprIds of the one-argument `R.split(S)` calls
    /// whose product is only ever measured (`analyze_split_length_only`).
    pub split_length_only_seps: std::collections::HashSet<ExprId>,
    /// The `R.trim()` calls likewise only measured (`analyze_trim_length_only`).
    pub trim_length_only_calls: std::collections::HashSet<ExprId>,
    /// The `.match` member ExprIds whose `const m = s.match(re)`
    /// result is only null-tested or asked `m[0].length`. Populated by
    /// `analyze_regex_match_len_only`; the lowering holds the first
    /// match's length instead of building the array.
    pub regex_match_len_callees: std::collections::HashSet<ExprId>,
    /// The `.exec` member ExprIds — of the declaration and of each
    /// re-assignment — whose `let m = re.exec(s)` result is only
    /// null-tested, read as `m.index` / `m.input`, indexed by a
    /// constant or measured as `m[k].length`, each mapped to the
    /// group slots the record needs. Populated by
    /// `analyze_regex_exec_record`; the lowering holds the match as
    /// spans in a stack record instead of building the array.
    pub regex_exec_record_callees: std::collections::HashMap<ExprId, u32>,
    /// The `.replace` / `.replaceAll` member ExprIds whose walk the
    /// lowering runs in the program's own code, each mapped to the
    /// view twin of its callback. Populated by
    /// `analyze_regex_replace_inline`.
    pub regex_replace_inline_callees: std::collections::HashMap<ExprId, String>,
    /// Lifted callback name → its view twin: the clone whose string
    /// parameters are Substr views, called only by that lane.
    pub regex_replace_view_fns: std::collections::HashMap<String, String>,
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
    /// `.cts` = CommonJS sloppy).
    ///
    /// This is the GOAL and nothing else. Almost every rule that reads
    /// it is written "in strict code", and a Script can be strict
    /// without changing goal — so read [`Ast::program_is_sloppy`]
    /// instead, unless the rule really does name the goal symbol (only
    /// §13.1.1 `await` does).
    pub sloppy_script_goal: bool,
    /// RFC 20260814 — the parser met at least one §14.11 `with` and
    /// emitted its `{ let __with_<n> = …; body }` shape. `desugar_with`
    /// is the first prelude pass and would otherwise pay a whole-tree
    /// walk for every program; `with` is sloppy-goal-only and rare, so
    /// the flag buys the common case out of it entirely.
    pub has_with_stmt: bool,
    /// RFC 20260912-global-var-cell — the names a script's top level
    /// declares with `var` (hoisted and escape-hatch forms alike),
    /// recorded by `var_hoist` under the script goal. §16.1.7 makes
    /// each an own property of the global object over the SAME
    /// storage: `collect_toplevel_globals` gives every one a data
    /// global (a cell needs a stable address) and the let-decl lane
    /// binds the slot to the object right after its store. The value
    /// says which attributes the property gets — a written `var` is
    /// not configurable, a synthesized implicit global is.
    pub script_global_var_names:
        std::collections::BTreeMap<String, super::var_hoist::ScriptGlobalVar>,
    /// RFC 20260912-global-var-cell knife 4 — the `var` sites the eval
    /// desugar inlined, by `init` slot, with the flavour its text gives:
    /// an eval's `var` is deletable (§19.2.1.3), a Script's is not
    /// (§16.1.7); a name the program declares stays a written `var`.
    /// After the hoist: the binding self-reads (`mark_eval_var_sites`).
    pub eval_var_sites: std::collections::HashMap<ExprId, super::var_hoist::ScriptGlobalVar>,
    /// RFC 20260912-global-var-cell knife 4 — the script-global names
    /// with NO bare compiled writer: no assignment / update names them
    /// anywhere and every `var` site is init-less. Over such a binding
    /// the object's attributes are the whole truth about writes, so the
    /// bind kernel may keep a read-only property and a redefine may set
    /// `writable: false` (`global_cell::KIND_FLAG_WRITERLESS`).
    pub script_global_writerless: std::collections::BTreeSet<String>,
    /// RFC 20260912-global-var-cell knife 4-3 — the top-level function
    /// declarations that came out of eval / Script text and are not
    /// also the program's own: `EvalFn` (deletable, §19.2.1.3) or
    /// `ScriptFn` (not, §16.1.7); both bind their property where the
    /// text ran (`script_global_fns` places the slot's assignment at
    /// the declaration's position instead of the prelude).
    pub eval_fn_names: std::collections::BTreeMap<String, super::var_hoist::ScriptGlobalVar>,
    /// Rotation 640 — the block-nested `function` names of eval / Script
    /// text, each with the flavour its Annex B §B.3.3 `var` binding
    /// takes (`EvalVar` / `ScriptVar`: site-bound, like every `var` of
    /// that text). The binding is minted by `desugar_nested_fns` long
    /// after the text was inlined, so its `init` cannot reach
    /// `eval_var_sites` from the inline; the lift files it there from
    /// this map. A name the program's own text also declares in a
    /// block is filed too and is harmless: its program-side `var` site
    /// keeps the name a written `var`.
    pub eval_annexb_fn_names: std::collections::BTreeMap<String, super::var_hoist::ScriptGlobalVar>,
    /// Rotation 640 — the names the program's top-level Scripts
    /// (`$262.evalScript`) declare lexically. §16.1.7 step 5.c: a
    /// binding an eval introduced is configurable, not a restricted
    /// global property, so a later Script's `let` / `const` of the name
    /// is legal and shadows it. The eval inliner drops such a
    /// declaration (`drop_script_shadowed_decls`) — flat, the two would
    /// be one name declared twice to the checker.
    pub script_lex_names: std::collections::HashSet<String>,
    /// What an eval's global instantiation needs (`EvalGlobalNames`).
    pub eval_global_names: super::var_hoist_eval_sites::EvalGlobalNames,
    /// RFC 20260825-injection-reachability — the parser met at least
    /// one `try` or `throw` statement, at ANY nesting depth. The
    /// injection reachability gate's catch-face door reads this flag:
    /// `ast.stmts` holds only the top level (statement bodies are
    /// inline Vecs, not arena ids), so an `.iter()` scan misses a
    /// try/catch inside a function body — the parser is the one
    /// place every statement passes through exactly once.
    pub has_try_or_throw: bool,
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
    /// Byte positions where the parser admitted `await` as a name with
    /// `[Await]` off — reserved under the module goal only, whatever the
    /// strictness. The goal bit is stamped after parsing, so
    /// `ast::triage_await_idents` (prelude) judges both lanes.
    pub await_ident_positions: Vec<u32>,
    /// Top-level `await` operators a Script cannot read as a name.
    pub module_await_positions: Vec<u32>,
    /// Every index signature and `Record<K, V>` the parser read, for
    /// the checker's diagnostics on its key and named members.
    pub dict_sites: Vec<DictSite>,
}

/// An index signature or a `Record<K, V>` as written: source text and
/// byte offsets of the key, the value and each named member beside it.
#[derive(Debug, Clone)]
pub struct DictSite {
    pub record: bool,
    pub key_at: u32,
    pub key: String,
    pub key_src: String,
    pub value: String,
    pub value_src: String,
    /// `(name, annotation, annotation source, name offset)`.
    pub members: Vec<(String, String, String, u32)>,
}

/// Thin wrapper preserving the `ast::desugar_builtin_new` public
/// surface for `torajs-cli` callers; body moved into
/// `ast_desugar_builtin_new` sibling (chunk 139).
pub fn desugar_builtin_new(ast: &mut Ast) {
    crate::ast_desugar_builtin_new::run(ast);
}

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
