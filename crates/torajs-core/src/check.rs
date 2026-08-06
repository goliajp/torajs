//! Type checker. Subset:
//! - primitives: `number`, `string`, `boolean`, `void`
//! - hardcoded `console: { log: any -> void }`
//! - top-level `function` declarations (hoisted, monomorphic)
//! - lexical scope stack (`let`/`const` block-scoped; fn params are a fresh scope)

use std::collections::HashMap;

use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};
use crate::lexer::Span;

mod diag;
mod process_on;
mod promise_static;
mod scope_moves;
mod stmt;
mod type_def;
mod type_of;

pub(crate) use diag::DiagPush;
pub use diag::{Diagnostic, Severity};
pub use type_def::Type;

pub(crate) type GenericAliasMap = HashMap<String, (Vec<String>, Vec<(String, String)>)>;

/// M6.1 — string / array methods that borrow both their receiver and
/// any args (no consume on pass). Shared between `check.rs`'s Call
/// arm (deciding whether to skip the move) and `ssa_lower`'s Member-
/// call dispatch (which routes these to the matching runtime
/// intrinsic). Single source of truth so the two layers can't drift
/// out of sync — a new borrow-method only has to be added here.
pub const STRING_BORROW_METHODS: &[&str] = &[
    "slice",
    "charCodeAt",
    "startsWith",
    "endsWith",
    "includes",
    "indexOf",
    "split",
    "join",
];

// `is_class_method_name` lives in [`crate::check_method_name`]
// (chunk-327 of the check.rs god-file decomp). Re-exported here so
// callers (`check_type_of_call_dispatch_flags`) keep the canonical
// `crate::check::is_class_method_name` import path.
pub use crate::check_method_name::is_class_method_name;

/// Per-scope-frame snapshot of which bindings are currently moved.
/// Each inner Vec captures the bindings inside one scope frame as
/// `(name, moved)` pairs in the order they happen to live in the
/// HashMap; the outer Vec is parallel to `Checker::scopes`.
pub(crate) type MovedSnapshot = Vec<Vec<(String, bool)>>;

/// True if `s` always exits its enclosing fn / loop / scope without
/// falling through. Used by CFG-aware moved tracking: a diverging
/// branch's local moves don't propagate to the post-branch state
/// (the moves go off with the diverging exit).
/// V3-18 wedge — true iff `s` (or anything reachable from it) is
/// an assignment to a top-level Ident binding `name`. Used by
/// the while-narrow guard to skip narrowing when the body
/// reassigns the binding (which would conflict with the
/// re-narrowing on the next iteration).
// `stmt_assigns_to` + the private `expr_assigns_to` helper live
// in [`crate::check_assigns_to`] (chunk-315 of the check.rs
// god-file decomp). Re-exported here so the external caller
// (`check_stmt_while`) continues to use the canonical
// `crate::check::stmt_assigns_to` import path.
pub(crate) use crate::check_assigns_to::{stmt_assigns_to, stmt_assigns_to_member};

// `stmt_diverges` lives in [`crate::check_stmt_diverges`] (chunk-317
// of the check.rs god-file decomp). Re-exported here so the external
// caller (`check_stmt_if`) continues to use the canonical
// `crate::check::stmt_diverges` import path.
pub(crate) use crate::check_stmt_diverges::stmt_diverges;

// The `unify_ternary` / `struct_is_prefix_subtype` pair (type-shape
// comparison family) lives in [`crate::check_type_compare`] (chunk-320
// of the check.rs god-file decomp). Re-exported here so callers
// (`check_type_of_ternary::unify_ternary`,
// `check_type_of_call_class_method_subtype::struct_is_prefix_subtype`)
// keep the canonical `crate::check::<name>` import path.
pub(crate) use crate::check_type_compare::{struct_is_prefix_subtype, unify_ternary};

// The `resolve_type_ann` / `build_fn_type` cluster (5 fn — 3 live thin
// wrappers + 2 dead `with_vars` variants kept under
// `#[allow(dead_code)]`) lives in [`crate::check_fn_type`] (chunk-319
// of the check.rs god-file decomp). Re-exported here so callers
// (`check_type_of_fn::build_fn_type`,
// `check_pipeline::{resolve_type_ann, build_fn_type_full}`) keep the
// canonical `crate::check::<name>` import path.
pub(crate) use crate::check_fn_type::{build_fn_type, build_fn_type_full, resolve_type_ann};

#[derive(Debug, Clone)]
pub(crate) struct LocalInfo {
    pub(crate) ty: Type,
    pub(crate) mutable: bool,
    /// Affine ownership flag. False until the binding's value is consumed
    /// (assign-rhs, non-Copy call-arg, struct-field write, return, throw).
    /// Reads of a moved binding still succeed (TS-shape); only a SECOND
    /// transfer errors. Copy-typed bindings never get marked.
    pub(crate) moved: bool,
    /// Alias flag — this binding borrows a heap owned elsewhere
    /// (Member / Index / cross-scope Ident init). Borrowed bindings
    /// can't be transferred at mid-scope sites (assign-rhs, call-arg,
    /// field write — ssa_lower has no retain contract there yet), but
    /// CAN escape via return / throw: the binding dies with the scope
    /// and ssa_lower retains at the boundary (retain-at-return /
    /// retain-at-throw) so the escaping reference carries its own +1.
    pub(crate) borrowed: bool,
    /// M-OO.5 — when this binding's declared type annotation matches a
    /// class name (`let c: Counter = ...`), record the class name so
    /// `c.member` accesses can look up the visibility entry in
    /// `ast.member_visibility`. Plain object-literal bindings, function-
    /// return bindings, and primitive bindings get `None` here. The
    /// nominal info lives on the binding rather than on `Type::Struct`
    /// to avoid a substrate refactor — it's enough for the
    /// `obj.member` pattern that visibility enforcement needs.
    pub(crate) declared_class: Option<String>,
    /// RFC 20260725-str-method-value-reify — this binding was
    /// initialized from a String-receiver builtin method VALUE read
    /// (`const m = s.slice`). Its `.call` / `.apply` / `.bind` admit
    /// as any-dispatched (the runtime re-dispatches the carried mid
    /// with spec-exact arity), so the member-table's fixed signature
    /// never rejects the optional-argument forms.
    pub(crate) builtin_mv: bool,
}

/// M3 — substitution recorded at each generic call site. Keyed by the
/// `Expr::Call`'s `ExprId`. Value: (callee fn name, ordered concrete
/// types matching the callee's `type_params`). The SSA monomorphizer
/// reads this to pick / generate the right specialized fn.
pub type GenericCallSites = HashMap<ExprId, (String, Vec<Type>)>;

// The JS-spec semantics acceptance family (5 fn: js_add_coerces_to_number,
// js_arith_coerces_to_number, is_known_builtin_global, js_truthy_acceptable,
// js_loose_eq_supported) lives in [`crate::check_js_semantics`] (chunk-321
// of the check.rs god-file decomp). Re-exported here so callers
// (`check_type_of_binop`, `check_type_of_ternary`, `check_type_of_unary`,
// `check_type_of_misc`, `check_stmt_while`, …) keep the
// `crate::check::<name>` import path.
pub(crate) use crate::check_js_semantics::{
    is_known_builtin_global, js_add_coerces_to_number, js_arith_coerces_to_number,
    js_loose_eq_supported, js_truthy_acceptable,
};

/// V3-05 — substitute `Type::ClassRef(name)` with whatever the
/// current `aliases[name]` is. Recurses through wrapper variants
/// (Nullable, Array) so a `Nullable(ClassRef("Node"))` field's
/// type resolves to `Nullable(Struct(node_real_fields))` for
/// downstream destructuring. Non-ClassRef types pass through
/// (cloned). Idempotent: resolving an already-resolved Type is
/// a no-op.
///
/// Use this at every site that needs to inspect the *shape* of a
/// type (field access, unify, member-call dispatch) — without it,
/// a self-referential class field stays at the placeholder and
/// the consumer sees no fields.
// `resolve_class_ref` + private helper `resolve_class_ref_one`
// live in [`crate::check_resolve_class_ref`] (chunk-316 of the
// check.rs god-file decomp). Re-exported here so callers
// continue to use the canonical `crate::check::resolve_class_ref`
// import path.
pub use crate::check_resolve_class_ref::resolve_class_ref;

// `type_to_ann` lives in [`crate::check_type_to_ann`] (chunk-314
// of the check.rs god-file decomp). Re-exported here so callers
// continue to use the canonical `crate::check::type_to_ann` path.
pub use crate::check_type_to_ann::type_to_ann;

// The six public entry-point fns + the `CheckArtifacts` tuple type
// live in [`crate::check_entry`] (chunk-318 of the check.rs god-file
// decomp). Re-exported here so external callers
// (`torajs_core::check::collect_diagnostics`,
// `torajs_core::check::check_with_arity`, etc.) keep their import
// paths working.
pub use crate::check_entry::{
    CheckArtifacts, check, check_with_arity, check_with_arity_warn, check_with_types,
    collect_diagnostics, collect_errors, collect_types_and_errors,
};

// Post-check monomorphization output (RFC
// 20260713-mono-check-specializations) — carried inside
// `CheckArtifacts`, so the type needs a public path.
pub use crate::check_monomorph::MonoOutput;

impl Checker {
    pub(crate) fn new() -> Self {
        Self {
            globals: HashMap::new(),
            hoisted_closure_lets: HashMap::new(),
            scopes: vec![HashMap::new()],
            aliases: HashMap::new(),
            class_structs: HashMap::new(),
            errors: Vec::new(),
            expected_return: None,
            current_class: None,
            closure_captures: HashMap::new(),
            closure_fn_names: std::collections::HashSet::new(),
            dynobj_degraded: std::collections::HashSet::new(),
            proto_shadow: Default::default(),
            cross_type_widened: std::collections::HashSet::new(),
            any_promoted_inits: std::collections::HashSet::new(),
            generic_type_params: HashMap::new(),
            generic_call_sites: HashMap::new(),
            any_widen_mono_sites: HashMap::new(),
            arity_pad_count: HashMap::new(),
            generic_alias_decls: HashMap::new(),
            fn_defaults: HashMap::new(),
            expr_types: HashMap::new(),
            demoted_cm_rewrites: HashMap::new(),
            contextual_any_literals: std::collections::HashSet::new(),
            iter_destr_srcs: HashMap::new(),
            undeclared_reads: std::collections::HashMap::new(),
            unresolved_captures: HashMap::new(),
            assign_narrows: HashMap::new(),
            member_narrows: HashMap::new(),
        }
    }

    pub(crate) fn run_full_pipeline(&mut self, ast: &Ast) {
        // RC-4 F1c — defineProperty receivers must type as `any` so
        // the dynobj write-back can rebind them (see
        // `crate::dynobj_degrade`). Collected before the passes so
        // the LetDecl arm sees the set.
        self.dynobj_degraded = crate::dynobj_degrade::collect_dynobj_degraded_inits(ast);
        // RFC 20260806 — what this module might patch onto a builtin
        // prototype, so a member read of such a method answers Any and
        // the call takes the dispatcher lane instead of a kernel.
        self.proto_shadow = crate::builtin_proto_shadow::collect_shadowed_builtin_methods(ast);
        // RFC 20260804-mutable-let-widen — cross-family reassigned
        // mutable lets type `any` from declaration (same shared-set
        // architecture; the lowerer recomputes from its own snapshot).
        self.cross_type_widened = crate::let_widen::collect_cross_type_widen_inits(ast);
        // Three native passes split out into `check_pipeline` sibling
        // (chunk 136). Order matters: Pass 1 reads aliases populated
        // by Pass 0; Pass 2 reads globals populated by Pass 1 and
        // the pre-pass.
        crate::check_pipeline::pass_0_register_type_aliases(self, ast);
        crate::check_pipeline::pass_1_hoist_fn_signatures(self, ast);
        crate::check_pipeline::pass_2_register_globals_and_check_stmts(self, ast);
    }
}

pub(crate) struct Checker {
    /// Builtin-prototype methods this module might shadow — a member
    /// read naming one answers `Any` so the call reaches the runtime
    /// dispatcher, which consults the patch bitmap and the delete
    /// tombstones (RFC 20260806). Empty for any program that never
    /// names a builtin prototype, which is the common case and costs
    /// one bool at each member read.
    pub(crate) proto_shadow: crate::builtin_proto_shadow::ShadowSet,
    pub(crate) globals: HashMap<String, Type>,
    pub(crate) scopes: Vec<HashMap<String, LocalInfo>>,
    /// Closure bindings a peer closure in the same statement list
    /// captures before their own declaration is reached — two arrows
    /// that call each other. A side table rather than a scope entry, so
    /// each binding's real declare stays a first declaration; see
    /// [`crate::check_hoist_closure_lets`]. Consulted only when
    /// resolving a closure's captures, after scopes and globals.
    pub(crate) hoisted_closure_lets: HashMap<String, Type>,
    /// User-declared type aliases — populated in pass 0 from
    /// `Stmt::TypeDecl`. `Point → Type::Struct(...)`.
    ///
    /// A CLASS keeps its `Type::ClassRef(name)` placeholder here (RFC
    /// 20260715-nominal-class-identity): TS is structural for
    /// ASSIGNABILITY but nominal for MEMBER OWNERSHIP, and a class
    /// whose type collapsed to its field struct let `{a: 1}` — the
    /// same shape — inherit `class C { a; get b() }`'s accessors and
    /// methods. Its resolved shape lives in [`Self::class_structs`].
    pub(crate) aliases: HashMap<String, Type>,
    /// The resolved field struct behind each class name. Separate from
    /// `aliases` so a class annotation resolves to `ClassRef(C)` (its
    /// nominal identity) while structural operations — field reads,
    /// assignability — still reach the shape via `resolve_class_ref`.
    pub(crate) class_structs: HashMap<String, Type>,
    /// T-04 — was `Vec<String>`; now carries severity + span. The
    /// public APIs (`check`, `collect_errors`) stringify back to the
    /// caller's expected shape; LSP consumes via `collect_diagnostics`.
    pub(crate) errors: Vec<Diagnostic>,
    pub(crate) expected_return: Option<Type>,
    /// M-OO.5 — when typechecking a fn body whose name follows the
    /// `__cm_<Class>__<method>` / `__sm_<Class>__<method>` shape that
    /// `desugar_classes` mints, `current_class` records the enclosing
    /// class. Member-access enforcement reads this to decide whether
    /// `obj.private_member` is allowed (private requires caller is
    /// inside the same class; protected requires caller in same class
    /// or descendant). `None` outside of class fn bodies (top-level
    /// stmts, free fns, etc.) — those treat every member as if it
    /// were public.
    pub(crate) current_class: Option<String>,
    /// M2 — captures for each lifted closure FnDecl. Populated by the
    /// `Expr::Closure` arm of `type_of` (which resolves capture types in
    /// the OUTER scope at the construction site) and consumed by the
    /// closure-FnDecl body walker below. Maps lifted-fn-name → ordered
    /// list of (capture_name, captured_type).
    pub(crate) closure_captures: HashMap<String, Vec<(String, Type)>>,
    /// Names of FnDecls that are lifted closures (first param is
    /// `__env`). Pass-2 skips these — their bodies are checked lazily
    /// when an `Expr::Closure` references them, so the captures are in
    /// scope.
    pub(crate) closure_fn_names: std::collections::HashSet<String>,
    /// RC-4 F1c — init ExprIds of the `let` declarations whose
    /// binding is the receiver of an `Object.defineProperty` /
    /// `defineProperties` call (`crate::dynobj_degrade` scope-correct
    /// walk). Such an unannotated `let x = { ... }` types as `any`
    /// so it lowers through the P3.2 dynobj-init lane and the define
    /// write-back can rebind it.
    pub(crate) dynobj_degraded: std::collections::HashSet<crate::ast::ExprId>,
    /// RFC 20260804-mutable-let-widen — init ExprIds of mutable
    /// lets later reassigned a different syntactic family
    /// (`crate::let_widen` scope-correct walk); such bindings type
    /// `any` from declaration.
    pub(crate) cross_type_widened: std::collections::HashSet<crate::ast::ExprId>,
    /// S2.35 — init ExprIds pass_2 promoted to an `Any` global via
    /// the shared [`crate::ast_refs_any_promote`] verdict (call-init
    /// / method-objlit). The LetDecl arm widens the main binding to
    /// `Any` off this set so both homes agree with the lowerer's
    /// Any slot (a typed main binding would route member calls to
    /// mono sigs the boxed slot value can never satisfy).
    pub(crate) any_promoted_inits: std::collections::HashSet<crate::ast::ExprId>,
    /// M3 — type params for each generic FnDecl (`function id<T, U>(...)`).
    /// Empty for non-generic fns. Pass-2 skips these decls (their
    /// TypeVar-bearing bodies can't be type-checked without substitution);
    /// the SSA monomorphization pass produces concrete bodies on demand.
    /// Call-site inference uses this map to walk each TypeVar in the
    /// signature and unify it against the actual argument type.
    pub(crate) generic_type_params: HashMap<String, Vec<String>>,
    /// M3 — per-call-site inferred type arguments. Keyed by the Call
    /// expression's `ExprId`; value is `(callee_name, ordered type args
    /// matching the callee's `type_params`)`. Read by the SSA monomorphizer
    /// at each generic call site to pick / generate the right specialized
    /// fn. Public via `pub fn check_with_generics` below.
    pub generic_call_sites: HashMap<ExprId, (String, Vec<Type>)>,
    /// RFC 20260802-any-arg-typed-param-mono — call sites whose
    /// `Array(Any)` argument was admitted against a typed `Array(T)`
    /// param of a plain-Ident user FnDecl. Keyed by the Call ExprId;
    /// value is `(callee_name, widened param indexes)`. Every entry
    /// MUST be retargeted by `check_monomorph_any_widen` to a clone
    /// whose params are widened to `any[]` — an admitted-but-not-
    /// retargeted site would hand a NaN-boxed Arr<Any> block to a
    /// typed slot reader (silent wrong values).
    pub any_widen_mono_sites: HashMap<ExprId, (String, Vec<usize>)>,
    /// T-28 — per-Call-site count of trailing args to pad with
    /// ANY_UNDEF. Set when caller passes fewer args than the callee's
    /// param count AND the trailing missing params are all Type::Any
    /// (per ES spec §10.2.1.4 — JS supplies undefined for missing).
    /// ssa_lower reads this and emits ANY_UNDEF Any-box operands at
    /// the trailing arg positions. Typed missing params still error
    /// at the arity check above so typed code keeps strict arity.
    pub arity_pad_count: HashMap<ExprId, usize>,
    /// M3.4 — generic struct alias declarations. Maps `Pair` →
    /// `(["A","B"], [("fst","A"), ("snd","B")])` for
    /// `type Pair<A, B> = { fst: A, snd: B }`. Used by
    /// `resolve_type_ann_with_vars` to instantiate `Pair<number|string>`
    /// on-demand into a concrete `Type::Struct`.
    pub(crate) generic_alias_decls: GenericAliasMap,
    /// Per-FnDecl default-value ExprIds in param order (None for
    /// required params, Some for `f(x = expr)`). Only present for fns
    /// with at least one defaulted param. Used at call-typecheck time
    /// to allow caller to omit trailing args, and at lower time to
    /// supply the default expr at the call site.
    pub fn_defaults: HashMap<String, Vec<Option<ExprId>>>,
    /// v0.3 #5 LSP — every successful `type_of(eid)` call records
    /// its result here so the LSP `hover` handler can answer "what
    /// is the type of the expression at this position?" by looking
    /// up the smallest-containing ExprId in the side table.
    /// Empty / unused outside the LSP entry point. Stays per-check
    /// (rebuilt each `collect_types_and_errors` call) so stale
    /// entries from edits don't surface.
    pub expr_types: HashMap<ExprId, Type>,
    /// Demoted speculative class-method rewrites, `call → alt
    /// member-call` ExprIds (single decision point — see cm_demote.rs).
    pub demoted_cm_rewrites: HashMap<ExprId, ExprId>,
    /// Chunk 702 — array literals whose Array<Any> type came from a
    /// let-decl annotation (contextual typing walker,
    /// check_stmt_let_decl::apply_contextual_array_ann). Lowering
    /// mints the FLAG_ARR_ANY flavor for exactly these; it must NOT
    /// key off `expr_types == Array<Any>` alone — the T-10.c
    /// infer-widen shapes (e.g. `["a", undefined]`) share that type
    /// but are deliberately taken by the typed lane (Str undefined
    /// sentinel slots).
    pub contextual_any_literals: std::collections::HashSet<ExprId>,
    /// RFC 20260714-dstr-residual blade 3 — the array-destructuring
    /// groups (`ast.ary_destr_groups`) whose source is not a statically
    /// indexable container, mapped to their step budget. Only the
    /// checker can tell: the parser sees `[a, b] = src` with no idea
    /// whether `src` is an array, a generator, or a Set. Carried to the
    /// lowerer on the post-check AST by `check_monomorph`.
    pub(crate) iter_destr_srcs: HashMap<ExprId, i64>,
    /// RFC 20260730-undeclared-ident — expression-position reads AND
    /// write-position targets that resolved nowhere (ExprId → name);
    /// typed `Any`, surfaced as a deduped end-of-pipeline warning,
    /// raised as runtime ReferenceError by the lowerer (§6.2.5.5
    /// GetValue / §6.2.5.6 PutValue are the same unresolvable
    /// Reference). A later resolution of the same eid unmarks it
    /// (speculative pre-pass typing self-heals). Carried to the
    /// lowerer on the post-check AST by `check_monomorph`.
    pub(crate) undeclared_reads: std::collections::HashMap<ExprId, String>,
    /// RFC 20260730-undeclared-ident 刀 3 — closure captures that
    /// resolved nowhere, keyed by the `Expr::Closure` construction
    /// site. `check_closure` skips them (the body's Ident read takes
    /// the undeclared-read lane instead); `check_monomorph` prunes
    /// them from the owned AST's capture lists so the lowerer's env
    /// materialization never sees them.
    pub(crate) unresolved_captures: HashMap<ExprId, Vec<String>>,
    /// Straight-line assignment-narrowing ledger — `name → declared
    /// (pre-narrow) type` for bindings narrowed by a statement-level
    /// `b = <non-null>` assign (see check_assign_narrow.rs). Flushed
    /// (restored to declared) at every compound-statement boundary,
    /// so a narrow never crosses a branch join or a loop back-edge.
    pub(crate) assign_narrows: HashMap<String, Type>,
    /// RFC 20260710 C5 — member-path truthiness narrows minted by an
    /// `if (o.cb) ...` guard: `(receiver ident, field name)` → the
    /// field's Nullable inner type. `check_type_of_member` consults
    /// this before the declared field type; any assignment to the
    /// receiver or the member invalidates its entry
    /// (`check_type_of_assign`).
    pub(crate) member_narrows: HashMap<(String, String), Type>,
}

// `is_array_method_name` lives in [`crate::check_method_name`]
// (chunk-327 of the check.rs god-file decomp). Re-exported here so
// callers (`check_type_of_member_array`,
// `check_type_of_member_prim_union`) keep the canonical
// `crate::check::is_array_method_name` import path.
pub(crate) use crate::check_method_name::is_array_method_name;

// `substitute_typevars` (M3 generic typevar substitution recursive
// walker) lives in [`crate::check_substitute_typevars`] (chunk-322 of
// the check.rs god-file decomp). Re-exported here so the external
// caller (`check_type_of_call_generic_ident`) keeps the canonical
// `crate::check::substitute_typevars` import path.
pub(crate) use crate::check_substitute_typevars::substitute_typevars;

#[cfg(test)]
mod tests {
    use crate::check::{Checker, Severity};
    use crate::lexer;
    use crate::parser;

    fn check_src(src: &str) -> Result<(), String> {
        let tokens = lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
        let mut ast = parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
        crate::ast::lift_arrow_fns(&mut ast);
        super::check(&ast).map(|_| ())
    }

    // M4 / M6 — review #0001 regression tests. Each one corresponds to
    // a P0/P1 bug fixed during the post-M4 phase review.

    #[test]
    fn try_finally_without_catch_parses() {
        // Parser accepts `try { } finally { }` (TS-legal since ES2019).
        let src = r#"
            function f(): number {
                try { return 1; } finally { console.log(0); }
            }
            f();
        "#;
        assert!(
            check_src(src).is_ok(),
            "expected ok, got {:?}",
            check_src(src)
        );
    }

    #[test]
    fn nested_capturing_closures_typecheck() {
        // Nested capturing closures used to crash in ssa_lower because
        // pass-2 lowered them in append (innermost-first) order.
        // typecheck-only here; the SSA lower order fix is in
        // ssa_lower's decl_indices reorder.
        let src = r#"
            function outer(a: number): number {
                let inner = (b: number): number => {
                    let inner2 = (c: number): number => a + b + c;
                    return inner2(100);
                };
                return inner(10);
            }
            outer(1);
        "#;
        assert!(
            check_src(src).is_ok(),
            "expected ok, got {:?}",
            check_src(src)
        );
    }

    #[test]
    fn return_inside_try_with_finally_typechecks() {
        // review #0001 fix — return inside try-with-finally now routes
        // through finally (was direct ret, skipping finally entirely).
        // Typechecks; the lowering changes are in ssa_lower.
        let src = r#"
            function f(): number {
                try { return 1; } catch (e) { return 99; }
                finally { console.log(0); }
            }
        "#;
        assert!(check_src(src).is_ok());
    }

    #[test]
    fn generic_fn_on_struct_arg_typechecks() {
        // `id<T>(x: T): T` applied to a struct used to fail because
        // type_to_ann returned "void" for Type::Struct, causing the
        // mono pass to lower a fn with void params (rejected by
        // both backends). Now encoded as `__struct(field:T|...)`.
        let src = r#"
            type A = { v: number };
            function id<T>(x: T): T { return x; }
            let a: A = { v: 5 };
            let a2: A = id(a);
            a2.v;
        "#;
        assert!(
            check_src(src).is_ok(),
            "expected ok, got {:?}",
            check_src(src)
        );
    }

    #[test]
    fn copy_types_can_be_used_repeatedly() {
        // number is Copy — using `n` after `let m = n` is fine.
        let src = "let n: number = 5; let m: number = n; let r: number = n + m;";
        assert!(
            check_src(src).is_ok(),
            "expected ok, got {:?}",
            check_src(src)
        );
    }

    #[test]
    fn shared_reads_after_let_alias_succeed() {
        // TS-shape: `let b = a` aliases the heap. Both `a` and `b` are
        // readable afterwards — the underlying string is the same value,
        // and end-of-scope drops it once via the b binding (a transferred
        // ownership at the let).
        let src = r#"
            let a: string = "hello";
            let b: string = a;
            let n: number = a.length;
            let m: number = b.length;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn shared_reads_after_assign_succeed() {
        // TS-shape: after `b = a`, both names alias the same heap.
        // a is still readable.
        let src = r#"
            let a: string = "x";
            let b: string = "y";
            b = a;
            let n: number = a.length;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn same_scope_copies_are_legal() {
        // Same-scope `let b = a` SHARES ownership (ssa_lower retains at
        // the binding site), so any number of copies is legal TS — no
        // affine transfer rule leaks into the surface.
        let src = r#"
            let a: string = "x";
            let b: string = a;
            let c: string = a;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn string_concat_does_not_consume_operands() {
        // TS-shape: `a + b` reads bytes from both, returns a fresh string.
        // Operands keep their heaps (matches bun) and remain readable.
        let src = r#"
            let a: string = "x";
            let b: string = "y";
            let c: string = a + b;
            let n: number = a.length;
            let m: number = b.length;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn console_log_does_not_move() {
        // `console.log` is special: it's a borrow-style viewer (Any param
        // sidesteps move-on-pass). Calling twice is fine.
        let src = r#"
            let a: string = "x";
            console.log(a);
            console.log(a);
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn copy_args_are_borrowed() {
        // number args don't get moved; the caller can still read after the call.
        let src = r#"
            function id(x: number): number { return x; }
            let n: number = 5;
            let m: number = id(n);
            let r: number = n + m;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_field_access_works() {
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 3, y: 4 };
            let n: number = p.x;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_alias_reads_succeed() {
        // TS-shape: `let q = p` aliases the same struct. Reading p's
        // fields after still works; one drop fires at end of scope (via q,
        // the current owner; p transferred).
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 3, y: 4 };
            let q: Point = p;
            let n: number = p.x;
            let m: number = q.y;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_same_scope_copies_are_legal() {
        // Same-scope struct copies share ownership like any other
        // refcounted value — `let q = p; let r = p` is legal TS.
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 3, y: 4 };
            let q: Point = p;
            let r: Point = p;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_field_type_must_resolve() {
        // Unknown type in field position errors at type-decl time.
        let src = "type Bad = { x: nope };";
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("unknown type"))
                .unwrap_or(false),
            "expected 'unknown type' error, got {r:?}"
        );
    }

    // ----- M2 Phase B Stage 1 — fn type annotation -----

    #[test]
    fn fn_type_annotation_typechecks() {
        let src = r#"
            function callFn(f: (n: number) => number, x: number): number {
              return f(x);
            }
            function double(x: number): number { return x * 2; }
            let n: number = callFn(double, 21);
        "#;
        // ssa_lower can't lower fn-type params yet (Stage 4 work);
        // we only verify check.rs accepts the annotation.
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn nested_fn_type_annotation_typechecks() {
        // (T) => U) => V — fn type can recurse.
        let src = r#"
            function apply(f: ((n: number) => number) => number, g: (m: number) => number): number {
              return f(g);
            }
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn fn_type_annotation_void_return_typechecks() {
        let src = r#"
            function each(xs: number[], f: (n: number) => void): void {
              return;
            }
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    // ----- M2 Phase A — non-capturing arrow fn -----

    #[test]
    fn arrow_fn_let_typechecks() {
        let src = r#"
            let double: number = 0;
            let f = (x: number): number => x * 2;
            let n: number = f(21);
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn arrow_fn_with_block_body_typechecks() {
        let src = r#"
            let f = (a: number, b: number): number => {
                return a + b;
            };
            let n: number = f(3, 4);
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn arrow_fn_capture_works() {
        // M2 — arrow fns now capture outer-scope idents (was rejected
        // in Phase A; M2 Phase C wired the env-block lowering).
        let src = r#"
            function outer(): number {
                let s: string = "captured";
                let f = (): number => s.length;
                return f();
            }
        "#;
        let r = check_src(src);
        assert!(r.is_ok(), "expected ok, got {r:?}");
    }

    // ----- M1.6 / M1.7 — for-loop + break/continue -----

    #[test]
    fn for_loop_typecheck_and_init_scope() {
        let src = r#"
            let total: number = 0;
            for (let i: number = 0; i < 10; i = i + 1) {
                total = total + i;
            }
            // i is out of scope here.
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn for_init_var_doesnt_leak_outer_scope() {
        // RFC 20260730-undeclared-ident — the out-of-scope read is no
        // longer a compile reject: it types Any, marks the read, and
        // defers to a runtime ReferenceError. The scope fact under
        // test (the for-init `i` does not leak) now shows up as the
        // surviving mark instead of an error.
        let src = r#"
            for (let i: number = 0; i < 10; i = i + 1) {
            }
            let n: number = i;
        "#;
        let tokens = lexer::tokenize(src).unwrap();
        let mut ast = parser::parse(src, &tokens).unwrap();
        crate::ast::lift_arrow_fns(&mut ast);
        let mut c = Checker::new();
        c.run_full_pipeline(&ast);
        assert!(
            !c.errors.iter().any(|d| d.severity == Severity::Error),
            "expected no hard errors, got {:?}",
            c.errors
        );
        assert!(
            c.undeclared_reads.values().any(|n| n == "i"),
            "expected the out-of-scope read of `i` to be marked, got {:?}",
            c.undeclared_reads
        );
    }

    #[test]
    fn break_continue_typecheck() {
        let src = r#"
            for (let i: number = 0; i < 100; i = i + 1) {
                if (i === 50) break;
                if (i % 2 === 0) continue;
            }
            let n: number = 0;
            while (n < 10) {
                n = n + 1;
                if (n === 5) break;
            }
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    // ----- M1.5 — boolean ops -----

    #[test]
    fn logical_and_or_not_typecheck() {
        let src = r#"
            let a: boolean = true;
            let b: boolean = false;
            let r1: boolean = a && b;
            let r2: boolean = a || b;
            let r3: boolean = !a;
            let r4: boolean = a && !b;
            let r5: boolean = (a || b) && !a;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn logical_op_on_non_bool_errors() {
        let src = "let n: number = 1; let r: boolean = n && true;";
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("`&&` / `||`") || s.contains("boolean"))
                .unwrap_or(false),
            "expected boolean-required error, got {r:?}"
        );
    }

    #[test]
    fn not_on_any_operand_is_boolean() {
        // Spec §13.5.7 (logical NOT) → §7.1.2 ToBoolean: `!x` is
        // defined for ANY operand type and always yields a boolean
        // (`!1 === false`, `!0 === true`). Applying `!` to a number
        // is NOT a type error — `let r: boolean = !n;` is valid TS
        // and bun runs it (verified byte-equal). The old assertion
        // pinned a pre-spec restriction the typechecker has since
        // correctly dropped; restoring the rejection would regress
        // test262. Spec-correct contract: this typechecks.
        let src = "let n: number = 1; let r: boolean = !n;";
        assert!(
            check_src(src).is_ok(),
            "`!<number>` is spec-valid (ToBoolean) and must typecheck, got {:?}",
            check_src(src)
        );
    }

    // ----- M1.4 — mutable struct field write + array index write -----

    #[test]
    fn struct_field_write_typechecks() {
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 1, y: 2 };
            p.x = 10;
            p.y = 20;
            let n: number = p.x;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_field_write_type_mismatch_errors() {
        let src = r#"
            type Point = { x: number, y: number };
            let p: Point = { x: 1, y: 2 };
            p.x = "hello";
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("type mismatch"))
                .unwrap_or(false),
            "expected type-mismatch error, got {r:?}"
        );
    }

    #[test]
    fn array_index_write_typechecks() {
        let src = r#"
            let xs: number[] = [];
            xs.push(0);
            xs[0] = 42;
            let n: number = xs[0];
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn array_index_write_type_mismatch_errors() {
        let src = r#"
            let xs: number[] = [];
            xs.push(0);
            xs[0] = "hello";
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("type mismatch"))
                .unwrap_or(false),
            "expected type-mismatch error, got {r:?}"
        );
    }

    // ----- M1.3 — block-scope drops + cross-scope alias -----

    #[test]
    fn cross_scope_let_is_alias_outer_still_readable() {
        // `let n = s` where s is in outer scope: under M1.3 this is
        // alias-only — n borrows s's heap, both readable, only one
        // drop fires (s at fn-end; n's slot is alias and skipped).
        let src = r#"
            function f(): number {
                let s: string = "outer";
                {
                    let n: string = s;
                    let m: number = n.length;
                }
                let len: number = s.length;
                return len;
            }
            console.log(f());
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn cross_scope_alias_does_not_consume_source() {
        // After cross-scope `let n = s; ...`, s should still be usable
        // via assign target without a "moved" error.
        let src = r#"
            function f(): number {
                let s: string = "x";
                {
                    let n: string = s;
                    let len: number = n.length;
                }
                let len2: number = s.length;
                return len2;
            }
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn same_scope_let_shares_and_source_stays_usable() {
        // Same-scope `let n = s` shares; s remains fully usable for
        // further copies and reads afterwards.
        let src = r#"
            let s: string = "x";
            let n: string = s;
            let m: string = s;
            let len: number = s.length;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn borrowed_alias_mid_scope_assign_shares_ok() {
        // A Member-init binding borrows obj's field heap. Assigning
        // it into another slot is a SHARE — ssa_lower's assign lane
        // retains borrow-shape rhs (+1, chunk 564), so the alias
        // keeps borrowing and the target owns its own stake.
        let src = r#"
            type Named = { name: string };
            let o: Named = { name: "n" };
            let n: string = o.name;
            let x: string = "y";
            x = n;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn line_comment_skipped() {
        let src = r#"
            // a comment at top
            let n: number = 5;  // a trailing comment
            // another full-line comment
            let m: number = n + 1;
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn block_comment_skipped() {
        let src = r#"
            /* leading block */
            let n: number = 5;
            /* multi
             * line
             * block */
            let m: number = n + 1;
            let s: string = "hi"; /* trailing on same line */
        "#;
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn unterminated_block_comment_errors() {
        let src = r#"let n: number = 5; /* unterminated"#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("unterminated block comment"))
                .unwrap_or(false),
            "expected unterminated-block-comment error, got {r:?}"
        );
    }

    #[test]
    fn slash_as_division_still_works() {
        // `/` not followed by `/` or `*` is still the division operator.
        let src = "let n: number = 10 / 2;";
        assert!(check_src(src).is_ok(), "got {:?}", check_src(src));
    }

    #[test]
    fn struct_forward_reference_ok() {
        // TS type aliases are not declaration-order sensitive — a
        // field may reference a sibling alias declared later. The old
        // test pinned a pre-spec "must declare first" limitation; the
        // checker now resolves forward refs correctly (verified
        // byte-equal with bun: `a.other.x` → 5). Rejecting valid
        // forward refs would regress test262. Spec-correct contract:
        // this typechecks.
        let src = r#"
            type A = { other: B };
            type B = { x: number };
            const b: B = { x: 5 };
            const a: A = { other: b };
        "#;
        assert!(
            check_src(src).is_ok(),
            "forward type-alias reference is valid TS and must typecheck, got {:?}",
            check_src(src)
        );
    }
}
