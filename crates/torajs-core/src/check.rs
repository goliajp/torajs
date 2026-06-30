//! Type checker. Subset:
//! - primitives: `number`, `string`, `boolean`, `void`
//! - hardcoded `console: { log: any -> void }`
//! - top-level `function` declarations (hoisted, monomorphic)
//! - lexical scope stack (`let`/`const` block-scoped; fn params are a fresh scope)

use std::collections::HashMap;

use crate::ast::{Ast, BinOp, Expr, ExprId, Param, Stmt};
use crate::lexer::Span;

mod process_on;
mod promise_static;

/// T-04 (v0.3.0) — typechecker diagnostic with source span + severity.
/// Replaces the previous `Vec<String>` error bucket. `span = (0, 0)`
/// is the sentinel for "no source location attached" — the LSP and
/// the CLI both render that as the file's first character. Per-site
/// span attachment lands incrementally as each push site gets an
/// ExprId in scope.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub span: Span,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Diagnostic {
    pub fn error(message: String) -> Self {
        Self {
            span: Span { start: 0, end: 0 },
            severity: Severity::Error,
            message,
        }
    }

    pub fn warning(message: String) -> Self {
        Self {
            span: Span { start: 0, end: 0 },
            severity: Severity::Warning,
            message,
        }
    }

    /// Same as `error` but attaches the source span of the offending
    /// `ExprId`. The Ast's `expr_spans` table is consulted; if the
    /// ExprId predates a span (common for synthesized exprs from the
    /// desugar passes), the diagnostic falls back to `(0, 0)`.
    pub fn error_at(ast: &Ast, eid: ExprId, message: String) -> Self {
        let span = ast
            .expr_spans
            .get(eid.0 as usize)
            .copied()
            .unwrap_or(Span { start: 0, end: 0 });
        Self {
            span,
            severity: Severity::Error,
            message,
        }
    }
}

/// T-04 — extension trait so the 35 `errors.push(format!(...))` sites
/// stay one mechanical rename (`push` → `push_err`) rather than each
/// growing a `Diagnostic::error(...)` wrapper. Future per-site span
/// attachment swaps `push_err(msg)` for `push_err_at(eid, msg)`.
pub(crate) trait DiagPush {
    fn push_err(&mut self, msg: String);
    #[allow(dead_code)] // T-06 lint will populate
    fn push_warn(&mut self, msg: String);
    /// Span-aware variant. Looks up the source span from `ast.expr_spans`.
    #[allow(dead_code)]
    fn push_err_at(&mut self, ast: &Ast, eid: ExprId, msg: String);
}

impl DiagPush for Vec<Diagnostic> {
    fn push_err(&mut self, msg: String) {
        self.push(Diagnostic::error(msg));
    }
    fn push_warn(&mut self, msg: String) {
        self.push(Diagnostic::warning(msg));
    }
    fn push_err_at(&mut self, ast: &Ast, eid: ExprId, msg: String) {
        self.push(Diagnostic::error_at(ast, eid, msg));
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Number,
    String,
    Boolean,
    Void,
    /// T-25 — arbitrary-precision integer (`123n` literal). Only
    /// equal to other BigInts. Cross-type ops with Number are a
    /// TypeError per spec; same-type ops produce BigInt.
    BigInt,
    /// T-26 — observed reference. `new WeakRef(target)` returns
    /// this; `wr.deref()` returns `Nullable<Any>` (type narrowing
    /// over generic T deferred — at the SSA layer the target slot
    /// is ptr-shaped regardless of the original T, and conformance
    /// cases work fine with `as` casts on the deref result).
    WeakRef,
    /// T-26.B — `WeakMap`. Pointer-identity-keyed map with auto-
    /// eviction on key death. `m.get(k)` returns `Nullable<Any>`,
    /// `m.has/delete` return Boolean, `m.set` returns the map (we
    /// stick with Void for the SSA result since chaining isn't
    /// observed in conformance fixtures yet — adjust if test262
    /// surfaces it).
    WeakMap,
    /// T-26.B — `WeakSet`. Set of pointer-identity-keyed entries
    /// with auto-eviction.
    WeakSet,
    /// P6.1 — `Map<K,V>`. Strong-ref hash map with SameValueZero
    /// key equality (string byte-equal / IEEE-754 number with NaN ==
    /// NaN / pointer identity for objects). `m.get(k)` returns
    /// `Nullable<V>`; `m.set/has/delete/clear` return Boolean / map /
    /// Number per spec.
    Map,
    /// P6.1 — `Set<T>`. Strong-ref hash set backed by `Map<T, undef>`.
    /// `s.add(v)` / `s.has(v)` / `s.delete(v)` / `s.size` per spec.
    Set,
    /// P6.4b — `MapIter`. Stateful iterator returned by
    /// `m.keys() / .values() / .entries()`. `iter.next()` returns
    /// `IteratorResult<any>` = `{ value: any, done: boolean }`.
    /// Holds a strong ref to the source Map.
    MapIter,
    /// P6.4c-C3 — `ArrIter`. Parallel to MapIter but scanning
    /// `Array<Any>` source. Returned by `arr.keys() / .values() /
    /// .entries()`. `iter.next()` returns the same
    /// `IteratorResult<any>` shape.
    ArrIter,
    /// v0 hack — `console.log`'s parameter accepts any printable type.
    /// Replace with a sum/union type later.
    Any,
    Function(Vec<Type>, Box<Type>),
    /// Hardcoded global stand-ins (currently only `console`). Real
    /// user-defined object types use `Type::Struct`.
    Object(&'static str),
    /// Homogeneous array. Owned by `Rc<Vec<Value>>` at runtime in v0.
    Array(Box<Type>),
    /// Structural object type — fields in declaration order. Two
    /// `Type::Struct` are equal iff they share field names + types in
    /// matching order. (TS-style structural compatibility, not nominal.)
    /// P2.4 introduced this; backed by heap allocation in P2.4.c.
    Struct(Vec<(String, Type)>),
    /// V3-05 — nominal class reference by name. Returned by
    /// `resolve_type_ann_full` when the type-ann names a declared
    /// class that's still in its pre-register placeholder phase
    /// (i.e. self / forward / mutual references during Pass 0). Use
    /// `resolve_class_ref(t, aliases)` at consumer sites to dereference
    /// to the current `aliases[name]` (which after Pass 0 is the
    /// real `Type::Struct(real_fields)`).
    ///
    /// Without ClassRef, `class Node { next: Node | null }` would
    /// embed a `Type::Struct(empty)` placeholder by-value into Node's
    /// own field — leading to unify mismatches when assigning a
    /// real Node into the field. ClassRef + lazy resolution avoids
    /// the by-value capture pitfall.
    ClassRef(String),
    /// M3 — type-parameter placeholder. Only legal inside the body of a
    /// generic FnDecl; at call sites the typechecker infers a concrete
    /// substitution and the `ssa_lower` monomorphization pass produces
    /// one specialized fn per `(name, type_args)` tuple. Two `TypeVar`s
    /// compare equal iff their names match, so distinct generic fns
    /// must use distinct param names (the per-fn alias scope makes
    /// this naturally true).
    TypeVar(String),
    /// `null` literal value. Only useful in unification with
    /// `Type::Nullable(T)` — a bare `let x: null = null` is legal but
    /// not very useful.
    Null,
    /// P1.1 — `undefined` literal value. Distinct from `Null` per
    /// ES spec §6.1.1 / §6.1.2. Currently behaves identically to
    /// `Null` everywhere except (eventually) typeof: `typeof undefined`
    /// must return `"undefined"` while `typeof null` returns `"object"`.
    /// Pre-P1.1 tora aliased `undefined` to `Null` end-to-end, which
    /// silently wrong-ed the typeof distinction; this variant is the
    /// substrate for fixing it incrementally without touching the
    /// 250+ Type::Null match arms in one big diff. The follow-up
    /// P1 sub-items (P1.5 typeof, P1.8 strict-eq, P1.3 default param,
    /// P1.4 OOB read) flip the per-arm behavior site-by-site.
    Undefined,
    /// `T | null` — pointer-shaped T may carry the in-band 0 sentinel
    /// at runtime. Restricted to T ∈ {String, Array<_>, Struct, …};
    /// number/boolean nullables would need a tag bit and aren't in v0.
    Nullable(Box<Type>),
    /// Compiled regex instance produced by a `/.../flags` literal or
    /// (eventually) `new RegExp(...)`. Heap-owned, non-Copy, ARC under
    /// the universal heap header. Distinct from `Type::Object("RegExp")`
    /// (the global constructor) — `RegExp` is the *value* type of `re`
    /// in `let re = /foo/i;`. Method dispatch (`.test`, `.exec`, ...)
    /// resolves through the Member arm against this variant.
    RegExp,
    /// Date instance produced by `new Date(...)` or
    /// `Date.parse(...)` (the latter returns Number, not Date).
    /// Heap-owned, non-Copy, ARC under the universal heap header.
    /// Underlying storage is `int64_t ms_since_epoch`. Method
    /// dispatch (`.getTime`, `.toISOString`, `.getFullYear`, ...)
    /// resolves through the Member arm against this variant.
    /// Distinct from `Type::Object("Date")` (the global constructor +
    /// static methods like `Date.now()`).
    Date,
    /// T-13.a (v0.4.0) — Symbol value. Each `Symbol(desc?)` call
    /// allocates a fresh heap block; identity is pointer identity.
    /// Heap-owned, non-Copy, ARC. Distinct from `Type::Object("Symbol")`
    /// (the constructor + future Symbol.for / well-known statics).
    /// `=== / !==` on Symbol operands compares ptr; console.log
    /// formats `Symbol(<desc>)`.
    Symbol,
    /// T-15 (v0.5.0) — built-in `Promise<T>` value. Heap-allocated
    /// 32-byte block managed by `runtime_promise.c`; carries state
    /// (PENDING / FULFILLED / REJECTED) + value + callback list.
    /// Heap-owned, non-Copy, ARC under the universal heap header
    /// (type_tag=8). Distinct from `Type::Object("Promise")` (the
    /// constructor + static methods like Promise.resolve).
    ///
    /// Type-system support shipped in T-15.f; runtime wiring follows
    /// in T-15.g (Promise.resolve / .then dispatch). Async function
    /// desugar (existing in ast.rs) will be migrated to use the
    /// built-in in T-15.h, replacing the user-class MVP that was
    /// the only async/await pattern through v0.4.0.
    Promise(Box<Type>),
}

impl Type {
    /// Cheap-to-duplicate types live entirely in registers / stack — using
    /// the binding twice just produces two independent copies with no
    /// runtime cost. Affine types own heap storage and follow Rust-shaped
    /// move semantics: each binding is the unique owner; consuming the
    /// binding (let-rhs / assign-rhs / call-arg / return) transfers
    /// ownership and the source name is marked moved.
    pub fn is_copy(&self) -> bool {
        matches!(self, Type::Number | Type::Boolean | Type::Void | Type::Any)
        // Struct, String, Function, Array — all heap-owned, all affine.
        // TypeVar — conservatively NOT Copy (instantiation may produce a
        // heap-owned type); irrelevant in practice since generic bodies
        // never witness their own end-of-fn drop walk (monomorphization
        // produces concrete-type bodies before the SSA layer runs).
    }
}

use crate::check_type_ann::resolve_type_ann_full;
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

/// M5.1 — class methods (and constructors) generated by `desugar_classes`
/// carry the `__cm_` prefix. The first argument of any such call is the
/// receiver (`__this`) and must be passed by borrow — neither check.rs's
/// affine consume nor ssa_lower's transfer/drop logic should fire on
/// arg[0] of these calls. Args[1..] follow the normal rules.
/// `__dispatch_` is the H.3.b synthetic virtual-dispatch entry; same
/// shape (arg[0] is borrow receiver, struct-prefix-subtype widening on
/// the call), so it shares the predicate.
pub fn is_class_method_name(name: &str) -> bool {
    name.starts_with("__cm_") || name.starts_with("__dispatch_")
}

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
pub(crate) use crate::check_assigns_to::stmt_assigns_to;

pub(crate) fn stmt_diverges(s: &crate::ast::Stmt) -> bool {
    use crate::ast::Stmt;
    match s {
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break | Stmt::Continue => true,
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.last().is_some_and(stmt_diverges),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            // If both branches diverge, the if as a whole diverges.
            stmt_diverges(then_branch) && else_branch.as_deref().is_some_and(stmt_diverges)
        }
        // While/For/DoWhile/Switch/Try/etc. could diverge in principle
        // (e.g. `while(true) { return ... }`) but we conservatively say
        // they don't — avoids false negatives on potentially-finite
        // loops. Worst case is we keep moves that should have been
        // discarded; the trailing post-loop code stays safe.
        _ => false,
    }
}

/// M5.2 — structural-prefix subtyping for class-method receiver arguments.
/// `Dog` (fields: name, bark_count) is a valid receiver for an
/// `Animal`-typed method (fields: name) because the `name` field sits at
/// the same offset in both layouts. We accept `arg` as a subtype of
/// `param` iff `param` is a `Struct` whose entire field list is a prefix
/// (in order, by name + type) of `arg`'s field list. Anything else falls
/// back to strict equality. Layout compatibility at the SSA / LLVM level
/// is the same — both are ptr to the heap-allocated obj header.
/// V3-18 wedge — ternary branch unification.
/// Returns the join type if `t` and `e` can unify, else None.
/// Rules:
///   - identical types unify to themselves
///   - `Null` and `T` unify to `Nullable<T>`
///   - `Nullable<T>` and `T` unify to `Nullable<T>`
///   - `Nullable<T>` and `Null` unify to `Nullable<T>`
pub(crate) fn unify_ternary(t: &Type, e: &Type) -> Option<Type> {
    if t == e {
        return Some(t.clone());
    }
    // S129-1 mixed-Any wedge — mirror of S128-5 LAnd/LOr / S127-4
    // indexOf 2-arg Any-escape. `b ? typed : (x: any)` widens the
    // result to Any; ssa-lower NaN-boxes the non-Any branch so the
    // shared __tern slot stays uniform.
    if matches!(t, Type::Any) || matches!(e, Type::Any) {
        return Some(Type::Any);
    }
    match (t, e) {
        (Type::Null, other) | (other, Type::Null) => Some(Type::Nullable(Box::new(other.clone()))),
        (Type::Nullable(inner), other) | (other, Type::Nullable(inner)) => {
            if inner.as_ref() == other {
                Some(Type::Nullable(inner.clone()))
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn struct_is_prefix_subtype(arg: &Type, param: &Type) -> bool {
    match (arg, param) {
        (Type::Struct(arg_fields), Type::Struct(param_fields)) => {
            if param_fields.len() > arg_fields.len() {
                return false;
            }
            for (i, (pn, pt)) in param_fields.iter().enumerate() {
                let (an, at) = &arg_fields[i];
                if an != pn || at != pt {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

#[allow(dead_code)]
pub(crate) fn resolve_type_ann(name: &str, aliases: &HashMap<String, Type>) -> Option<Type> {
    resolve_type_ann_full(name, aliases, &[], &HashMap::new())
}

/// Like `resolve_type_ann`, but accepts a slice of in-scope type-parameter
/// names. A bare identifier matching one of those resolves to
/// `Type::TypeVar(name)` regardless of any conflicting alias / primitive.
/// Used by `build_fn_type` for generic fn bodies. M3.
#[allow(dead_code)]
fn resolve_type_ann_with_vars(
    name: &str,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
) -> Option<Type> {
    resolve_type_ann_full(name, aliases, type_params, &HashMap::new())
}

/// Full resolver: also accepts a generic-alias-decls map so `Pair<X|Y>`
/// instantiates against the original `type Pair<A, B> = { ... }` decl.
/// M3.4.
pub(crate) fn build_fn_type(
    fn_name: &str,
    params: &[Param],
    return_type: &Option<String>,
    aliases: &HashMap<String, Type>,
) -> Result<Type, String> {
    build_fn_type_with_vars(fn_name, params, return_type, aliases, &[])
}

fn build_fn_type_with_vars(
    fn_name: &str,
    params: &[Param],
    return_type: &Option<String>,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
) -> Result<Type, String> {
    build_fn_type_full(
        fn_name,
        params,
        return_type,
        aliases,
        type_params,
        &HashMap::new(),
    )
}

pub(crate) fn build_fn_type_full(
    fn_name: &str,
    params: &[Param],
    return_type: &Option<String>,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
) -> Result<Type, String> {
    let mut param_tys = Vec::new();
    for p in params {
        let Some(ann) = &p.type_ann else {
            return Err(format!(
                "parameter `{}` of function `{fn_name}` requires a type annotation",
                p.name
            ));
        };
        let Some(ty) = resolve_type_ann_full(ann, aliases, type_params, generic_aliases) else {
            return Err(format!(
                "unknown type `{ann}` for parameter `{}` of function `{fn_name}`",
                p.name
            ));
        };
        param_tys.push(ty);
    }
    let ret_ty = match return_type {
        None => Type::Void,
        Some(t) => match resolve_type_ann_full(t, aliases, type_params, generic_aliases) {
            Some(ty) => ty,
            None => {
                return Err(format!(
                    "unknown return type `{t}` for function `{fn_name}`"
                ));
            }
        },
    };
    Ok(Type::Function(param_tys, Box::new(ret_ty)))
}

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
}

/// M3 — substitution recorded at each generic call site. Keyed by the
/// `Expr::Call`'s `ExprId`. Value: (callee fn name, ordered concrete
/// types matching the callee's `type_params`). The SSA monomorphizer
/// reads this to pick / generate the right specialized fn.
pub type GenericCallSites = HashMap<ExprId, (String, Vec<Type>)>;

/// V3-18 m1.a — JS spec §13.15.3 ApplyStringOrNumericBinaryOperator
/// guard for non-string `+`. Returns true iff both operands are
/// statically-typed numerics-or-coercible-to-numerics: Number,
/// Boolean (ToNumber → 0/1), Null (ToNumber → 0). When this holds,
/// check.rs accepts `l + r` with result type Number; ssa_lower
/// mirrors the coercion at lower time. Strings are handled by the
/// existing String+String / String+Number arms (they short-circuit
/// before this helper). Excludes BigInt — mixed BigInt+Number is a
/// TypeError per spec, caught by the trailing catch-all.
pub(crate) fn js_add_coerces_to_number(l: &Type, r: &Type) -> bool {
    fn coerces(t: &Type) -> bool {
        matches!(t, Type::Number | Type::Boolean | Type::Null)
    }
    coerces(l) && coerces(r) && (*l != Type::Number || *r != Type::Number)
}

/// V3-18 m1.b — `-` / `*` / `/` / `%` use the same ToNumber rule
/// as `+` but never have a String-concat path (spec §13.7-§13.10
/// unconditionally call ToNumeric). Same coercibles as m1.a.
pub(crate) fn js_arith_coerces_to_number(l: &Type, r: &Type) -> bool {
    js_add_coerces_to_number(l, r)
}

/// V3-18 m1.h.3 — built-in JS globals that tora's check.rs
/// resolves via the special Ident → Type::Object("X") arms.
/// Used by the `typeof undeclared` path to avoid mis-classifying
/// a tora built-in as "undefined".
pub(crate) fn is_known_builtin_global(name: &str) -> bool {
    matches!(
        name,
        "console"
            | "Math"
            | "Object"
            | "Number"
            | "String"
            | "JSON"
            | "Array"
            | "Date"
            | "WeakRef"
            | "WeakMap"
            | "WeakSet"
            | "Symbol"
            | "BigInt"
            | "Boolean"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "ReferenceError"
            | "SyntaxError"
            | "Bun"
            | "Promise"
            | "RegExp"
            | "Map"
            | "Set"
            | "fetch"
            | "process"
            | "globalThis"
            | "undefined"
            | "NaN"
            | "Infinity"
            | "encodeURI"
            | "decodeURI"
            | "encodeURIComponent"
            | "decodeURIComponent"
            | "parseInt"
            | "parseFloat"
            | "isFinite"
            | "isNaN"
            | "Function"
            | "eval"
            // WHATWG HTML microtask scheduling (P10.1)
            | "queueMicrotask"
    )
}

/// V3-18 m1.h.1 — JS spec §7.1.2 ToBoolean acceptance check.
/// Used at every condition site (if / while / do-while / for /
/// ternary). Anything except Void is coercible to bool — Number
/// (0/NaN→false), String (""→false), Null (→false), Object/etc
/// (always-true). ssa_lower routes the cond through
/// `coerce_to_bool` to emit the actual coercion at lower time.
pub(crate) fn js_truthy_acceptable(t: &Type) -> bool {
    !matches!(t, Type::Void)
}

/// V3-18 m3 — `==` / `!=` cross-type pair guard. Spec §7.2.13:
///   Number == Boolean → coerce Boolean
///   Boolean == Number → coerce Boolean
///   Number == Null    → false  (not coerced)
///   Null   == Boolean → false
///   ...
/// We accept any pair of Number/Boolean/Null types — the runtime
/// coercion (or static-false for null vs non-null-non-undefined)
/// happens at lower time. String / BigInt / Object cross-type
/// pairs go through ToPrimitive → numeric and ship in a later
/// wedge.
pub(crate) fn js_loose_eq_supported(l: &Type, r: &Type) -> bool {
    if matches!(l, Type::Number | Type::Boolean | Type::Null)
        && matches!(r, Type::Number | Type::Boolean | Type::Null)
    {
        return true;
    }
    // ES §7.2.13 steps 2-4 — Null / Undefined never coerce to other
    // primitive types for loose-eq, so the cross-type result is
    // statically false (unless the other side is also nullish, in
    // which case the result is true). Accept Undefined symmetric with
    // Null in the same nullish bucket so the type-check passes; SSA
    // lowering's `a_is_null || b_is_null` arm (ssa_lower.rs L25533)
    // already folds to `ConstBool(false)` for cross-type and
    // `ConstBool(true)` for nullish-nullish — same fold both literals
    // hit since they share `Operand::ConstPtrNull`. Other side may be
    // any primitive (Number / Boolean / String / BigInt) — the fold
    // doesn't need to inspect it.
    let nullish = |t: &Type| matches!(t, Type::Null | Type::Undefined);
    let primitive = |t: &Type| {
        matches!(
            t,
            Type::Number
                | Type::Boolean
                | Type::Null
                | Type::Undefined
                | Type::String
                | Type::BigInt
        )
    };
    if (nullish(l) && primitive(r)) || (nullish(r) && primitive(l)) {
        return true;
    }
    // ES §7.2.13 step 6 — String <-> Number cross-pair coerces the
    // string side via ToNumber + numeric compare. ssa_lower
    // (loose-eq arm following the nullish fold) emits
    // `__torajs_str_to_number` + fcmp.
    if (matches!(l, Type::String) && matches!(r, Type::Number))
        || (matches!(l, Type::Number) && matches!(r, Type::String))
    {
        return true;
    }
    // ES §7.2.13 steps 7-9 — Boolean <-> String coerces both sides
    // to Number (Boolean → 0/1, String → ToNumber). ssa_lower
    // (loose-eq arm sibling to the Str/Number arm) emits the
    // compound coercion + fcmp.
    if (matches!(l, Type::String) && matches!(r, Type::Boolean))
        || (matches!(l, Type::Boolean) && matches!(r, Type::String))
    {
        return true;
    }
    // S127-2 — Any vs Null (covers both `null` and `undefined` literals,
    // which both lower to Type::Null at check time). spec §7.2.13
    // IsLooselyEqual: `v == null` is true iff `v` is null or undefined,
    // for any runtime type of `v`. ssa_lower folds this into a nullish
    // check via `any_strict_eq_imm_pair` against tag=0 (ANY_NULL) and
    // tag=5 (ANY_UNDEF). The standard idiom `v == null` is widespread in
    // callbacks over Array<Any> (.some / .every / .find / .reduce).
    matches!((l, r), (Type::Any, Type::Null) | (Type::Null, Type::Any))
}

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
pub fn resolve_class_ref(
    ty: &Type,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> Type {
    match ty {
        Type::ClassRef(name) => {
            match aliases.get(name) {
                Some(t) if !matches!(t, Type::ClassRef(_)) => {
                    // Recurse: the alias entry's own fields may
                    // themselves contain ClassRef placeholders (the
                    // self-ref case — Node's `next` field carries
                    // ClassRef("Node")). One unwrap pass keeps
                    // following levels resolved at access time.
                    let resolved = t.clone();
                    resolve_class_ref_one(&resolved, aliases, generic_aliases)
                }
                // Generic-instantiation back-edge (`Rec<number>` from a
                // recursive `type Rec<T>`): the key is not in `aliases`
                // (instantiation is lazy, there is no Pass-0 entry) but
                // it IS its own canonical annotation — re-instantiate
                // one shallow layer. Recursive fields inside come back
                // as ClassRef again, so this terminates: same lazy
                // one-layer-per-access contract as the named-class arm.
                None if name.contains('<') => {
                    match resolve_type_ann_full(name, aliases, &[], generic_aliases) {
                        Some(t) if !matches!(t, Type::ClassRef(_)) => t,
                        _ => ty.clone(),
                    }
                }
                _ => ty.clone(),
            }
        }
        _ => resolve_class_ref_one(ty, aliases, generic_aliases),
    }
}

/// Helper: walk every wrapper variant once, leaving ClassRef nodes
/// embedded in struct/array fields alone (they get resolved on the
/// next access). This keeps recursive class layouts finite — a
/// fully-resolved Node would expand infinitely.
fn resolve_class_ref_one(
    ty: &Type,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> Type {
    match ty {
        Type::Nullable(inner) => {
            Type::Nullable(Box::new(resolve_class_ref(inner, aliases, generic_aliases)))
        }
        Type::Array(inner) => {
            Type::Array(Box::new(resolve_class_ref(inner, aliases, generic_aliases)))
        }
        _ => ty.clone(),
    }
}

// `type_to_ann` lives in [`crate::check_type_to_ann`] (chunk-314
// of the check.rs god-file decomp). Re-exported here so callers
// continue to use the canonical `crate::check::type_to_ann` path.
pub use crate::check_type_to_ann::type_to_ann;

pub fn check(ast: &Ast) -> Result<GenericCallSites, String> {
    check_with_types(ast).map(|(g, _)| g)
}

/// T-15.g.6 (v0.5.0) — typed variant of `check`. Also returns the
/// per-Expr type map so ssa_lower can recover Promise<T>'s inner T
/// at the await Member-access site (Type::Promise is unit at SSA;
/// PromiseId interning would be the cleaner fix but threads through
/// 22+ parse_type call sites — this side-channel is the smaller
/// change).
pub fn check_with_types(ast: &Ast) -> Result<(GenericCallSites, HashMap<ExprId, Type>), String> {
    check_with_arity(ast).map(|(g, t, _, _)| (g, t))
}

/// T-28 — full pipeline artifacts: generic call sites, per-Expr types,
/// per-Call arity pad map, and the demoted speculative-rewrite map
/// (name-based class-method rewrites whose receiver checked as a
/// builtin container; ssa_lower restores the member-call shape for
/// those — see cm_demote.rs).
pub type CheckArtifacts = (
    GenericCallSites,
    HashMap<ExprId, Type>,
    HashMap<ExprId, usize>,
    HashMap<ExprId, ExprId>,
);

/// T-28 — check that also returns the per-Call arity pad + demotion
/// maps. New callers (main.rs `tr run` / `tr build`) use this; the
/// older `check_with_types` is kept for back-compat (tests, lsp).
pub fn check_with_arity(ast: &Ast) -> Result<CheckArtifacts, String> {
    let mut c = Checker::new();
    c.run_full_pipeline(ast);
    let error_messages: Vec<String> = c
        .errors
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    if error_messages.is_empty() {
        Ok((
            c.generic_call_sites,
            c.expr_types,
            c.arity_pad_count,
            c.demoted_cm_rewrites,
        ))
    } else {
        Err(error_messages.join("\n"))
    }
}

/// v0.3 #5 LSP — string-typed errors-only collector kept for back-
/// compat. Filters out warnings so callers that historically only
/// surfaced errors keep the same shape. New callers should prefer
/// `collect_diagnostics` (T-04).
pub fn collect_errors(ast: &Ast) -> Vec<String> {
    let mut c = Checker::new();
    c.run_full_pipeline(ast);
    c.errors
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect()
}

/// T-04 (v0.3.0) — full diagnostic stream with source spans + severity.
/// LSP consumes this to publish per-site squiggles + warning bucket
/// (lint also reads this same stream once it lands in T-06).
pub fn collect_diagnostics(ast: &Ast) -> Vec<Diagnostic> {
    let mut c = Checker::new();
    c.run_full_pipeline(ast);
    c.errors
}

/// v0.3 #5 LSP L-3 — run the full typecheck pipeline and return
/// the per-Expr type table (populated as a side-effect by every
/// `type_of` call). Caller looks up by ExprId. Errors during
/// typecheck don't abort the table — partial coverage on the
/// reachable Exprs is still useful for hover. Errors are stringified
/// (errors-only, no warnings) for back-compat with existing LSP
/// hover code; full diagnostics flow through `collect_diagnostics`.
pub fn collect_types_and_errors(ast: &Ast) -> (HashMap<ExprId, Type>, Vec<String>) {
    let mut c = Checker::new();
    c.run_full_pipeline(ast);
    let errs: Vec<String> = c
        .errors
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect();
    (c.expr_types, errs)
}

impl Checker {
    fn new() -> Self {
        Self {
            globals: HashMap::new(),
            scopes: vec![HashMap::new()],
            aliases: HashMap::new(),
            errors: Vec::new(),
            expected_return: None,
            current_class: None,
            closure_captures: HashMap::new(),
            closure_fn_names: std::collections::HashSet::new(),
            generic_type_params: HashMap::new(),
            generic_call_sites: HashMap::new(),
            arity_pad_count: HashMap::new(),
            generic_alias_decls: HashMap::new(),
            fn_defaults: HashMap::new(),
            consumed_calls: std::collections::HashSet::new(),
            expr_types: HashMap::new(),
            demoted_cm_rewrites: HashMap::new(),
        }
    }

    fn run_full_pipeline(&mut self, ast: &Ast) {
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
    pub(crate) globals: HashMap<String, Type>,
    pub(crate) scopes: Vec<HashMap<String, LocalInfo>>,
    /// User-declared type aliases — populated in pass 0 from
    /// `Stmt::TypeDecl`. `Point → Type::Struct(...)`.
    pub(crate) aliases: HashMap<String, Type>,
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
    /// Set of `Expr::Call` ExprIds whose consume-bitmap has already
    /// fired. type_of() is called multiple times on the same Call expr
    /// in some flow paths (e.g. Stmt::Throw runs type_of, then runs it
    /// again to fetch the type for consume); without this guard a Call
    /// inside the throw expression would consume its args twice and
    /// trip the affine tracker on the second pass.
    pub(crate) consumed_calls: std::collections::HashSet<ExprId>,
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
}

/// T-29 — built-in Array.prototype method names recognized by the
/// per-method Call-dispatch in check.rs. The Member-on-Array
/// catch-all uses this to avoid shadowing them with Type::Any when
/// a bare `arr.method` access appears outside a call site.
pub(crate) fn is_array_method_name(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "slice"
            | "splice"
            | "concat"
            | "join"
            | "reverse"
            | "sort"
            | "indexOf"
            | "lastIndexOf"
            | "includes"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "map"
            | "filter"
            | "reduce"
            | "reduceRight"
            | "forEach"
            | "every"
            | "some"
            | "flat"
            | "flatMap"
            | "fill"
            | "copyWithin"
            | "at"
            | "entries"
            | "keys"
            | "values"
            | "toString"
            | "toLocaleString"
            | "toReversed"
            | "toSorted"
            | "toSpliced"
            | "with"
            | "indexOfStartingAt"
    )
}

pub(crate) fn substitute_typevars(ty: &Type, subst: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TypeVar(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Array(inner) => Type::Array(Box::new(substitute_typevars(inner, subst))),
        Type::Function(args, ret) => Type::Function(
            args.iter().map(|t| substitute_typevars(t, subst)).collect(),
            Box::new(substitute_typevars(ret, subst)),
        ),
        Type::Struct(fields) => Type::Struct(
            fields
                .iter()
                .map(|(n, t)| (n.clone(), substitute_typevars(t, subst)))
                .collect(),
        ),
        other => other.clone(),
    }
}

impl Checker {
    pub(crate) fn declare(&mut self, name: String, info: LocalInfo) -> Result<(), String> {
        let top = self
            .scopes
            .last_mut()
            .expect("at least one scope is always present");
        if top.contains_key(&name) {
            return Err(format!("redeclaration of `{name}` in current scope"));
        }
        top.insert(name, info);
        Ok(())
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<LocalInfo> {
        for s in self.scopes.iter().rev() {
            if let Some(i) = s.get(name) {
                return Some(i.clone());
            }
        }
        None
    }

    /// V3-18 wedge — detect a flow-narrowing cond shape on the
    /// form `<ident> !== null` / `null !== <ident>` (and === for
    /// the inverse polarity). Returns (binding-name, inner-type,
    /// then-narrows). Polarity = true means the then-branch
    /// narrows, false means the else-branch.
    pub(crate) fn collect_null_narrow(
        &self,
        ast: &Ast,
        cond: ExprId,
    ) -> Option<(String, Type, bool)> {
        // Cond shape 1 — `<ident> !== null` / `null !== <ident>`
        // (and `===` for the inverse polarity). The historical
        // narrow shape, kept verbatim.
        if let Expr::BinOp { op, left, right } = ast.get_expr(cond) {
            let polarity = match op {
                BinOp::Neq | BinOp::LooseNeq => Some(true),
                BinOp::Eq | BinOp::LooseEq => Some(false),
                _ => None,
            };
            if let Some(polarity) = polarity {
                let name = match (ast.get_expr(*left), ast.get_expr(*right)) {
                    (Expr::Ident(n), Expr::Null) => Some(n.clone()),
                    (Expr::Null, Expr::Ident(n)) => Some(n.clone()),
                    _ => None,
                };
                if let Some(name) = name {
                    let info = self.lookup(&name)?;
                    if let Type::Nullable(inner) = info.ty.clone() {
                        return Some((name, *inner, polarity));
                    }
                }
            }
        }
        // Cond shape 2 (truthy-narrow wedge) — bare ident or
        // `!ident` where ident is Nullable<T>. Per JS spec
        // §7.1.2 ToBoolean, `null` is falsy, so `if (s) ...`
        // narrows the then-branch to T (or the else-branch via
        // `!s`). For Nullable<Number> the then-branch also
        // excludes 0 and for Nullable<String> it excludes "",
        // but that just makes the value *more* constrained — it
        // is still a valid T, which is all the narrow promises.
        // Other primitives (number, string, boolean, struct) on
        // their own are not Nullable here, so this hook only
        // fires when the binding's declared type is Nullable.
        let (target, polarity) = match ast.get_expr(cond) {
            Expr::Ident(n) => (n.clone(), true),
            Expr::Unary {
                op: crate::ast::UnaryOp::Not,
                expr,
            } => {
                if let Expr::Ident(n) = ast.get_expr(*expr) {
                    (n.clone(), false)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        let info = self.lookup(&target)?;
        if let Type::Nullable(inner) = info.ty.clone() {
            Some((target, *inner, polarity))
        } else {
            None
        }
    }

    /// V3-18 wedge — narrow the binding `name` to `inner_ty`
    /// in the innermost scope that owns it; return the previous
    /// type so it can be restored after the narrowed branch.
    pub(crate) fn apply_narrow(&mut self, name: &str, inner_ty: Type) -> Option<Type> {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                let prev = info.ty.clone();
                info.ty = inner_ty;
                return Some(prev);
            }
        }
        None
    }

    pub(crate) fn restore_narrow(&mut self, name: &str, prev_ty: Type) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.ty = prev_ty;
                return;
            }
        }
    }

    /// Like `lookup` but also returns the scope depth at which the binding
    /// was found (0 = outermost / fn-root, `scopes.len() - 1` = innermost).
    /// M1.3 uses this to detect cross-scope `let n = s` cases — an Ident
    /// init from an outer scope is treated as alias-only (n borrows s's
    /// heap, both stay readable; no ownership transfer that would dangle
    /// the outer reference at this block's close).
    fn lookup_with_depth(&self, name: &str) -> Option<(LocalInfo, usize)> {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if let Some(info) = s.get(name) {
                return Some((info.clone(), i));
            }
        }
        None
    }

    /// M-OO.5 — true iff `child` is a descendant of `ancestor` along
    /// the class inheritance chain stored in `ast.class_parents`.
    /// Used by Protected visibility enforcement: `protected member`
    /// access is allowed when the caller's class is the owner OR any
    /// subclass.
    pub(crate) fn is_descendant_of(&self, ast: &Ast, child: &str, ancestor: &str) -> bool {
        let mut cur = child;
        while let Some(parent) = ast.class_parents.get(cur).and_then(|p| p.as_deref()) {
            if parent == ancestor {
                return true;
            }
            cur = parent;
        }
        false
    }

    /// Walk the scope stack from innermost outward and flip `moved=true`
    /// on the first matching binding. Caller must already have verified
    /// the binding exists.
    fn mark_moved(&mut self, name: &str) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.moved = true;
                return;
            }
        }
    }

    /// Inverse of `mark_moved` — the binding's slot now owns a fresh value
    /// (Assign rebound it). Used to clear any transient `moved` state set
    /// during rhs evaluation. Lets `s = s + "x"` work: the BinOp internally
    /// consumes s (because str+str consumes both), then Assign rebinds s
    /// with the concat result, so subsequent reads of s are fine.
    pub(crate) fn mark_unmoved(&mut self, name: &str) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.moved = false;
                return;
            }
        }
    }

    /// Snapshot every (scope_idx, name) → moved bool across the whole
    /// scope stack. Used by CFG-aware branch checking: snapshot before
    /// a branch, run the branch (which may mark bindings moved), then
    /// either restore the snapshot (diverging branch) or merge the
    /// captured post-state with sibling branches' post-states.
    pub(crate) fn snapshot_moved(&self) -> Vec<Vec<(String, bool)>> {
        self.scopes
            .iter()
            .map(|s| s.iter().map(|(n, i)| (n.clone(), i.moved)).collect())
            .collect()
    }

    /// Restore moved flags to the values captured by `snapshot_moved`.
    /// Bindings introduced after the snapshot (i.e. inside the branch)
    /// are unaffected — they're either still in the scope or already
    /// popped by branch teardown.
    pub(crate) fn restore_moved(&mut self, snap: &[Vec<(String, bool)>]) {
        for (scope, snap_scope) in self.scopes.iter_mut().zip(snap.iter()) {
            for (n, m) in snap_scope {
                if let Some(info) = scope.get_mut(n) {
                    info.moved = *m;
                }
            }
        }
    }

    /// Apply the join of two branches' post-move states to the current
    /// scope stack. A binding is marked moved post-join iff every
    /// non-diverging branch moved it. Diverging branches contribute no
    /// post-join moves (their moves go off with the diverging exit).
    /// `pre` is the snapshot taken before either branch ran; `then_post`
    /// / `else_post` are the snapshots taken after each branch ran (or
    /// None for an absent else, which is treated as "live, no moves").
    pub(crate) fn join_branch_moves(
        &mut self,
        pre: &[Vec<(String, bool)>],
        then_post: &[Vec<(String, bool)>],
        then_div: bool,
        else_post: Option<&[Vec<(String, bool)>]>,
        else_div: bool,
    ) {
        // For each scope frame and binding, compute newly-moved-in-branch
        // (post.moved && !pre.moved) for each side, then join.
        for (scope_idx, pre_scope) in pre.iter().enumerate() {
            for (name, pre_moved) in pre_scope {
                if *pre_moved {
                    // Already moved before the if; nothing changes.
                    continue;
                }
                let then_moved = then_post.get(scope_idx).is_some_and(|s| {
                    s.iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, m)| *m)
                        .unwrap_or(false)
                });
                let else_moved = match else_post {
                    Some(es) => es.get(scope_idx).is_some_and(|s| {
                        s.iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, m)| *m)
                            .unwrap_or(false)
                    }),
                    // Absent else = implicit empty path that didn't move.
                    None => false,
                };
                let then_lives = !then_div;
                let else_lives = match else_post {
                    Some(_) => !else_div,
                    None => true,
                };
                let join_moved = match (then_lives, else_lives) {
                    // Both diverge → post-if unreachable. Pre-state survives.
                    (false, false) => *pre_moved,
                    // Only else lives → propagate else's moves.
                    (false, true) => else_moved,
                    // Only then lives → propagate then's moves.
                    (true, false) => then_moved,
                    // Both live → conservative intersection (require both
                    // sides to consume for post-state to be moved).
                    (true, true) => then_moved && else_moved,
                };
                if join_moved && !pre_moved {
                    // Mark in the right scope (we know it's at scope_idx).
                    if let Some(scope) = self.scopes.get_mut(scope_idx)
                        && let Some(info) = scope.get_mut(name)
                    {
                        info.moved = true;
                    }
                }
            }
        }
    }

    /// Try to transfer ownership FROM the given expression. Called at the
    /// four transfer sites: let-rhs, assign-rhs, non-Copy fn arg, return
    /// value, struct field write.
    ///
    /// TS-shape semantics: `let n = s; console.log(s);` works — both
    /// bindings read the same heap. But ambiguous multi-rooted ownership
    /// (`let n = s; let c = { name: s };` — s aliased AND moved into struct)
    /// can't be statically resolved without a runtime mechanism we don't
    /// have, so we **reject at compile time**: the second transfer of an
    /// already-aliased binding is an error. The user restructures (e.g.
    /// transfers from `n` instead of `s`).
    ///
    /// Member / Index reads of obj's field are NOT transfers — the field's
    /// heap is owned by obj, and the new binding is an alias (handled at
    /// the LetDecl site via `classify_init_alias`, not here).
    pub(crate) fn consume(&mut self, ast: &Ast, eid: ExprId) {
        if let Expr::Ident(name) = ast.get_expr(eid) {
            let name = name.clone();
            if let Some(info) = self.lookup(&name) {
                if info.ty.is_copy() {
                    return;
                }
                if info.borrowed {
                    self.errors.push_err(format!(
                        "cannot transfer `{name}` — it aliases a value owned elsewhere; transfer from the owning binding instead"
                    ));
                    return;
                }
                if info.moved {
                    self.errors.push_err(format!(
                        "cannot transfer `{name}` — value was already aliased or moved earlier; transfer from the most recent binding instead"
                    ));
                    return;
                }
                self.mark_moved(&name);
            }
        }
    }

    /// Transfer at a scope-exit boundary (return / throw). Unlike the
    /// mid-scope `consume`, a borrowed alias is legal here: the binding
    /// dies with the scope and ssa_lower retains at the boundary
    /// (retain-at-return / retain-at-throw), so the escaping reference
    /// carries its own +1 while the canonical owner keeps its stake.
    /// Owned bindings still consume — their stake transfers out.
    pub(crate) fn consume_escape(&mut self, ast: &Ast, eid: ExprId) {
        if let Expr::Ident(name) = ast.get_expr(eid)
            && let Some(info) = self.lookup(name)
            && info.borrowed
            && !info.ty.is_copy()
        {
            return;
        }
        self.consume(ast, eid);
    }

    /// Decide whether a let-bound or struct-field's init expression
    /// produces a fresh-owned value or aliases an existing one. Member
    /// and Index reads (`obj.field`, `arr[i]`) yield aliases — the heap
    /// is still owned by obj/arr; the new binding just holds a pointer
    /// for shared-read access. M1.3 extends this to cross-scope Ident
    /// init: when `s` lives in an outer scope, `let n = s` becomes an
    /// alias (otherwise transferring would dangle the outer reference
    /// when the inner block's drop fires). Same-scope Ident init is a
    /// SHARE — ssa_lower retains at the binding site so both bindings
    /// own independent stakes; neither is an alias nor consumed.
    /// Fresh-value init (literal, Call return, BinOp, ObjectLit, Array)
    /// produces a new owner; not an alias.
    pub(crate) fn classify_init_alias(&self, ast: &Ast, eid: ExprId) -> bool {
        match ast.get_expr(eid) {
            Expr::Member { .. } | Expr::Index { .. } => true,
            Expr::Ident(name) => {
                if let Some((_, src_depth)) = self.lookup_with_depth(name) {
                    let cur_depth = self.scopes.len() - 1;
                    src_depth < cur_depth
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub(crate) fn check_stmt(&mut self, ast: &Ast, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(eid) => {
                if let Err(e) = self.type_of(ast, *eid) {
                    self.errors.push_err(e);
                }
            }
            Stmt::Yield(_) | Stmt::YieldInto { .. } => {
                // Phase J — desugar_generators rewrites generator
                // bodies before typecheck. See
                // [`crate::check_stmt_misc::check_yield`].
                crate::check_stmt_misc::check_yield(self);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                // V3-18 narrow wedge + CFG-aware moved snapshot +
                // post-if narrow on diverge. See
                // [`crate::check_stmt_if::check`].
                crate::check_stmt_if::check(self, ast, *cond, then_branch, else_branch);
            }
            Stmt::While { cond, body } => {
                // V3-18 narrow wedge (gated on no-reassign body).
                // See [`crate::check_stmt_while::check_while`].
                crate::check_stmt_while::check_while(self, ast, *cond, body);
            }
            Stmt::DoWhile { body, cond } => {
                // Body runs first; cond typechecks after.
                // See [`crate::check_stmt_while::check_do_while`].
                crate::check_stmt_while::check_do_while(self, ast, body, *cond);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                // Scrutinee type vs case value type; nested scope per
                // case body + default. See
                // [`crate::check_stmt_misc::check_switch`].
                crate::check_stmt_misc::check_switch(self, ast, *scrutinee, cases, default);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                // Fresh-scope init / cond / step / body walker. See
                // [`crate::check_stmt_for::check`].
                crate::check_stmt_for::check(self, ast, init, cond, step, body);
            }
            Stmt::Throw(eid) => {
                // M4.3 + P7.2a + P4.7 — accept 8-byte-shaped
                // throw value; consume non-Copy via
                // consume_escape. See
                // [`crate::check_stmt_throw::check`].
                crate::check_stmt_throw::check(self, ast, *eid);
            }
            Stmt::Try {
                body,
                had_catch: _,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                // 3-phase walker (body / catch with P7.2b-2 Any
                // default / optional finally). See
                // [`crate::check_stmt_try::check`].
                crate::check_stmt_try::check(
                    self,
                    ast,
                    body,
                    catch_param,
                    catch_type,
                    catch_body,
                    finally_body,
                );
            }
            Stmt::Break | Stmt::Continue => {
                // No type-side state to track; the lowerer enforces that
                // these only appear inside loops.
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                // P-iter — parent/sep typecheck + var_name Substr-
                // shaped String borrow per iteration. See
                // [`crate::check_stmt_for_of_split::check`].
                crate::check_stmt_for_of_split::check(self, ast, var_name, *parent, *sep, body);
            }
            // P5.3 — generic for-of. The parser hoists src to a fresh
            // Ident and pre-builds `elem_expr = src[i]`. Typing the
            // var_name binding goes through Expr::Index lowering on
            // elem_expr — which already infers the right element type
            // per source shape (Array<T>.value=T, String[i]=String,
            // dynobj-backed Any[i]=Any). We also declare `i_ident` as
            // a Number local so the synthetic counter typechecks.
            //
            // P5.3 Phase B exception: when src has Type::Struct (i.e.
            // a class instance), the protocol path in ssa_lower
            // bypasses elem_expr entirely — typing `src[i]` here
            // would error ("can't index into Struct"). We probe src's
            // type first; if it's a class-shape Struct, defer the
            // element type to ssa_lower (mark as Any so var_name still
            // typechecks downstream as opaque).
            Stmt::ForOf {
                var_name,
                var_type_ann,
                src_ident: _,
                i_ident,
                elem_expr,
                body,
            } => {
                // P5.3 generic for-of + P5.3 Phase B Struct skip +
                // P6.4c Map/Set/MapIter/ArrIter skip. See
                // [`crate::check_stmt_for_of::check`].
                crate::check_stmt_for_of::check(
                    self,
                    ast,
                    var_name,
                    var_type_ann,
                    i_ident,
                    *elem_expr,
                    body,
                );
            }
            Stmt::Block(stmts) => {
                self.scopes.push(HashMap::new());
                for s in stmts {
                    self.check_stmt(ast, s);
                }
                self.scopes.pop();
            }
            Stmt::Multi(stmts) => {
                // Surrounding scope shared — no push.
                for s in stmts {
                    self.check_stmt(ast, s);
                }
            }
            // `is_var` is intentionally ignored here: `desugar_var_hoist`
            // runs before check and rewrites every `var` into a
            // hoisted `let`-shaped decl (is_var: false), so the
            // checker never observes a true `var` — not a silent-wrong,
            // var semantics are fully resolved upstream.
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var: _,
            } => {
                // M1.2 + P0.10 empty-array narrow + annotation
                // assignability + alias classify + M-OO.5 nominal
                // info + LocalInfo declare. See
                // [`crate::check_stmt_let_decl::check`].
                crate::check_stmt_let_decl::check(self, ast, *mutable, name, type_ann, *init);
            }
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                // M-OO.5 class context + fresh-scope body walker +
                // param declare with declared_class propagation. See
                // [`crate::check_stmt_fn_decl::check`].
                crate::check_stmt_fn_decl::check(self, ast, name, params, body);
            }
            Stmt::TypeDecl { .. } => {
                // Already handled in pass 0; re-encountering it during the
                // body walk is a no-op. (No nested type decls — top-level
                // only — but the AST shape allows them anywhere.)
            }
            Stmt::Return(maybe_expr) => {
                // expected_return + Nullable<T> wedge +
                // P0.9 assignability lattice + move-out
                // (consume_escape for non-Copy). See
                // [`crate::check_stmt_return::check`].
                crate::check_stmt_return::check(self, ast, *maybe_expr);
            }
            // M5.1 — desugar_classes runs before check, so by the time we
            // walk the AST every ClassDecl has been split into a TypeDecl
            // + a series of FnDecls. Reaching here means the desugar pass
            // missed something — treat as an internal-error panic instead
            // of producing a bogus "type error".
            Stmt::ClassDecl { name, .. } => {
                panic!("internal: ClassDecl `{name}` reached check.rs (desugar didn't run?)");
            }
            Stmt::ImportDecl { .. } => {
                // K.1 single-file mode: import is parse-only, no
                // semantic effect. K.2 will add the cross-file symbol
                // table check here.
            }
            Stmt::ExportDecl { inner, .. } => {
                // K.1 single-file mode: export is the modifier wrapper;
                // typecheck the wrapped declaration if any.
                if let Some(inner) = inner {
                    self.check_stmt(ast, inner);
                }
            }
        }
    }

    /// v0.3 #5 LSP — outer wrapper that records every successful
    /// type-of result into `expr_types` so the LSP hover handler
    /// can answer position queries without re-running the typecheck
    /// pipeline. Recursive calls hit this same wrapper, so deeply
    /// nested Exprs all get their inferred type cached.
    pub(crate) fn type_of(&mut self, ast: &Ast, eid: ExprId) -> Result<Type, String> {
        let result = self.type_of_inner(ast, eid);
        if let Ok(t) = &result {
            self.expr_types.insert(eid, t.clone());
        }
        result
    }

    fn type_of_inner(&mut self, ast: &Ast, eid: ExprId) -> Result<Type, String> {
        match ast.get_expr(eid) {
            // P4.5 — `new.target` is Type::Any. Inside a ctor body
            // desugar_classes rewrites to Ident("__new_target") which
            // typechecks against the synthesized param's type. Outside
            // ctors the bare NewTarget reaches here and resolves to
            // Any (spec §13.3.10 says `undefined` outside ctor;
            // tagged ANY_UNDEF at the SSA layer).
            Expr::NewTarget => Ok(Type::Any),
            Expr::String(_) => Ok(Type::String),
            Expr::Number(_) => Ok(Type::Number),
            Expr::BigInt { .. } => Ok(Type::BigInt),
            Expr::Bool(_) => Ok(Type::Boolean),
            Expr::Null => Ok(Type::Null),
            // P1.3 — `let x;` (no init) gives x the value `undefined`
            // per ES spec §8.1 / §14.3.2. Pre-P1 tora returned
            // Type::Null (collapsed with undefined at the runtime).
            // Now Type::Undefined first-class: typeof an uninit
            // binding correctly returns "undefined" and strict-eq
            // distinguishes from null. Still resolved by
            // `desugar_uninit_let` to the first follow-up assignment's
            // RHS when one exists; the Type::Undefined fallback only
            // fires for genuinely uninit slots.
            Expr::Uninit => Ok(Type::Undefined),
            // Regex literal `/pat/flags` — produces a `Type::RegExp`.
            // Pattern + flags are validated at runtime in
            // `__torajs_regex_compile` (allocates the NFA + flag bits);
            // the typechecker only confirms the literal is well-shaped.
            // Method dispatch (`.test`, `.exec`, ...) is resolved
            // through the Member arm against `Type::RegExp`.
            Expr::Regex {
                pattern: _,
                flags: _,
            } => Ok(Type::RegExp),
            Expr::Ident(name) => {
                // Resolution order: local binding → module global →
                // built-in namespace (console/Math/Object/Array/...) →
                // synthesized intrinsic fn (__torajs_*) → manual GC /
                // distinguished literal (undefined / NaN / Infinity) →
                // unknown identifier error. See
                // [`crate::check_type_of_ident::check`].
                crate::check_type_of_ident::check(self, name)
            }
            Expr::Member { obj, name } => crate::check_type_of_member::check(self, ast, obj, name),
            Expr::Index { obj, index } => {
                // V3-18 index-expression — `obj[index]` Number-index
                // narrow + String/Array<T> receiver narrow. See
                // [`crate::check_type_of_index::check`].
                crate::check_type_of_index::check(self, ast, *obj, *index)
            }
            Expr::Array(elements) => {
                // T-10.c / P0.10 / S134 / S141 — array literal
                // typecheck (empty default Array<Any>; per-element
                // value type incl. spread sources Array<T>/String/Set;
                // anchor + heterogeneous → Array<Any> widen). See
                // [`crate::check_type_of_array::check`].
                crate::check_type_of_array::check(self, ast, elements)
            }
            Expr::Spread { .. } => Err("spread `...` is only valid inside an array literal".into()),
            Expr::ObjectLit { fields } => {
                // ObjectLit type-build: spread member sentinel
                // `__spread__` unfolds source struct fields;
                // inline members win on key collision; final
                // type is freshly-merged Type::Struct. See
                // [`crate::check_type_of_object_lit::check`].
                crate::check_type_of_object_lit::check(self, ast, fields)
            }
            Expr::Call { callee, args } => {
                crate::check_type_of_call::check(self, ast, eid, callee, args)
            }
            Expr::BinOp { op, left, right } => {
                // 8 op-family per-shape rules (Add /
                // Sub-Mul-Div-Mod / Pow / Bitwise / UShr /
                // Lt-Gt-Le-Ge / Eq-Neq-LooseEq-LooseNeq /
                // LAnd-LOr). See
                // [`crate::check_type_of_binop::check`].
                crate::check_type_of_binop::check(self, ast, *op, *left, *right)
            }
            Expr::Unary { op, expr } => {
                // V3-18 m1.h.2 / m1.f / m1.h.4 / V3-02 — `!` / `-`
                // / `+` / `~` per-op type rules (ToBoolean for `!`;
                // ToNumber/Int32 for the rest; BigInt special-cases;
                // Any → Any). See
                // [`crate::check_type_of_unary::check`].
                crate::check_type_of_unary::check(self, ast, *op, *expr)
            }
            Expr::Assign { target, value } => {
                // Target-shape dispatch (Ident / Member / Index /
                // invalid). See
                // [`crate::check_type_of_assign::check`].
                crate::check_type_of_assign::check(self, ast, *target, *value)
            }
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                // Non-capturing arrow (lift_arrow_fns survivor). See
                // [`crate::check_type_of_fn::check_arrow_fn`].
                crate::check_type_of_fn::check_arrow_fn(self, ast, params, return_type, body)
            }
            Expr::Closure { fn_name, captures } => {
                // Lifted FnDecl with capture types resolved in outer
                // scope + lazy body walk. See
                // [`crate::check_type_of_fn::check_closure`].
                crate::check_type_of_fn::check_closure(self, ast, fn_name, captures)
            }
            // M5.1 — desugar_classes flattens these out before check runs.
            // Reaching here is an internal compiler error, not a user error.
            Expr::This => panic!("internal: bare `this` reached check.rs (desugar didn't run?)"),
            Expr::New { class_name, args } => {
                // 7 built-in classes (WeakRef / WeakMap / WeakSet /
                // Map / Set / Array / RegExp) with their constructor
                // arg shape rules — user classes are pre-rewritten
                // to `__new_<C>(…)` Call by `desugar_classes`. See
                // [`crate::check_type_of_new::try_check`].
                if let Some(r) = crate::check_type_of_new::try_check(self, ast, class_name, args) {
                    return r;
                }
                panic!("internal: `new {class_name}` reached check.rs (desugar didn't run?)")
            }
            Expr::Super { .. } => {
                panic!("internal: `super(...)` reached check.rs (desugar didn't run?)")
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                // V3-18 ternary-narrow wedge + Nullable<T>
                // branch widen. See
                // [`crate::check_type_of_ternary::check`].
                crate::check_type_of_ternary::check(self, ast, *cond, *then_branch, *else_branch)
            }
            Expr::TypeOf { expr } => {
                // V3-18 m1.h.3 / m1.h.20 — `typeof expr` → String
                // with known-builtin Ident / namespace-member short-
                // circuit. See
                // [`crate::check_type_of_misc::check_typeof`].
                crate::check_type_of_misc::check_typeof(self, ast, *expr)
            }
            Expr::InstanceOf { expr, .. } => {
                // `x instanceof C` → Boolean. See
                // [`crate::check_type_of_misc::check_instanceof`].
                crate::check_type_of_misc::check_instanceof(self, ast, *expr)
            }
            Expr::Nullish { lhs, rhs } => {
                // `lhs ?? rhs` — Nullable/Null/Undef/Any lhs +
                // rhs T or Nullable<T>; non-nullable lhs → lhs
                // type. See
                // [`crate::check_type_of_misc::check_nullish`].
                crate::check_type_of_misc::check_nullish(self, ast, *lhs, *rhs)
            }
            Expr::OptChain { obj, name } => {
                // P3.5 — `obj?.field` returns Type::Any per ES
                // §13.3.9 (Nullable/Null/Undefined/Any obj); plain
                // obj → concrete member type. See
                // [`crate::check_type_of_misc::check_opt_chain`].
                crate::check_type_of_misc::check_opt_chain(self, ast, *obj, name)
            }
            Expr::PostIncr { target, .. } => {
                // `x++` / `x--` — Number target → Number result.
                // See [`crate::check_type_of_misc::check_post_incr`].
                crate::check_type_of_misc::check_post_incr(self, ast, *target)
            }
            // V3-18 m1.h.6 — comma operator `(a, b)` evaluates left
            // (side effects, value discarded) then right; result type
            // = right's type. Both sub-expressions still type-checked.
            Expr::Sequence { left, right } => {
                // V3-18 m1.h.6 — comma operator. See
                // [`crate::check_type_of_misc::check_sequence`].
                crate::check_type_of_misc::check_sequence(self, ast, *left, *right)
            }
            // V3-07 — `expr as T` TS type assertion. Typecheck the
            // inner expression for side effects (so it still
            // participates in move tracking + sub-expression validation),
            // then return the asserted target type. Spec-strict TS
            // narrows assertion compatibility (`x as Foo` requires
            // either side to be assignable to the other); we accept
            // unconditionally for now — matches `as any` widening +
            // the common downcast pattern. Full bidirectional
            // assignability check lands when test262 surfaces a case
            // that requires it.
            Expr::As { expr, ty_ann } => {
                // V3-07 / V3-18 — TS type assertion + non-null
                // assertion (`x!` encoded ty_ann = "__nonnull__").
                // See [`crate::check_type_of_misc::check_as`].
                crate::check_type_of_misc::check_as(self, ast, *expr, ty_ann)
            }
        }
    }

    /// Look up `name` on `obj_ty` and return the field/method type.
    /// Pulled out so OptChain can reuse Member's resolution logic
    /// without re-implementing the alias / array / class / Math /
    /// console branches.
    pub(crate) fn member_type(&mut self, obj_ty: &Type, name: &str) -> Result<Type, String> {
        match (obj_ty, name) {
            (Type::String, "length") | (Type::Array(_), "length") => Ok(Type::Number),
            /* v0.3 #3 — process.platform constant string read. */
            (Type::Object("process"), "platform") => Ok(Type::String),
            (Type::Object("process"), "argv") | (Type::Object("Bun"), "argv") => {
                Ok(Type::Array(Box::new(Type::String)))
            }
            /* `process.env` — returns the env namespace, used as
             * the receiver for further `process.env.NAME` access. */
            (Type::Object("process"), "env") => Ok(Type::Object("env")),
            /* `process.env.NAME` — runtime getenv, Nullable<String>. */
            (Type::Object("env"), _name) => Ok(Type::Nullable(Box::new(Type::String))),
            /* T-03 (v0.3.0) — process.{stdout, stderr, stdin} expose
             * their own Object so `.write` / `.read` resolve at the
             * Call type-check arm (see member-Call dispatch above). */
            (Type::Object("process"), "stdout") => Ok(Type::Object("process_stdout")),
            (Type::Object("process"), "stderr") => Ok(Type::Object("process_stderr")),
            (Type::Object("process"), "stdin") => Ok(Type::Object("process_stdin")),
            (Type::Struct(fields), n) => fields
                .iter()
                .find(|(fn_, _)| fn_ == n)
                .map(|(_, t)| t.clone())
                .ok_or_else(|| format!("no field `{n}` on struct {obj_ty:?}")),
            (other, _) => Err(format!("no field `{name}` accessible on type {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer;
    use crate::parser;

    fn check_src(src: &str) -> Result<(), String> {
        let tokens = lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
        let mut ast = parser::parse(&tokens).map_err(|e| format!("parse: {e}"))?;
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
        let src = r#"
            for (let i: number = 0; i < 10; i = i + 1) {
            }
            let n: number = i;
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("unknown identifier `i`"))
                .unwrap_or(false),
            "expected scope-leak error, got {r:?}"
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
    fn borrowed_alias_mid_scope_transfer_errors() {
        // A Member-init binding borrows obj's field heap. Assigning it
        // into another slot is a mid-scope transfer of a borrowed
        // reference — ssa_lower has no retain contract at assign-rhs
        // yet, so check rejects it (escape via return/throw stays
        // legal — see consume_escape).
        let src = r#"
            type Named = { name: string };
            let o: Named = { name: "n" };
            let n: string = o.name;
            let x: string = "y";
            x = n;
        "#;
        let r = check_src(src);
        assert!(
            r.as_ref()
                .err()
                .map(|s| s.contains("cannot transfer"))
                .unwrap_or(false),
            "expected borrowed-transfer error, got {r:?}"
        );
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
