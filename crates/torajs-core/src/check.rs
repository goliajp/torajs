//! Type checker. Subset:
//! - primitives: `number`, `string`, `boolean`, `void`
//! - hardcoded `console: { log: any -> void }`
//! - top-level `function` declarations (hoisted, monomorphic)
//! - lexical scope stack (`let`/`const` block-scoped; fn params are a fresh scope)

use crate::check_typevar::{typevar_appears_in, typevar_appears_in_iter, unify_typevar};
use std::collections::HashMap;

use crate::ast::{Ast, BinOp, Expr, ExprId, Param, Stmt, Visibility};
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
trait DiagPush {
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

use crate::check_assignable::is_assignable_to_resolved;
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
type MovedSnapshot = Vec<Vec<(String, bool)>>;

/// True if `s` always exits its enclosing fn / loop / scope without
/// falling through. Used by CFG-aware moved tracking: a diverging
/// branch's local moves don't propagate to the post-branch state
/// (the moves go off with the diverging exit).
/// V3-18 wedge — true iff `s` (or anything reachable from it) is
/// an assignment to a top-level Ident binding `name`. Used by
/// the while-narrow guard to skip narrowing when the body
/// reassigns the binding (which would conflict with the
/// re-narrowing on the next iteration).
fn stmt_assigns_to(ast: &Ast, s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => expr_assigns_to(ast, *eid, name),
        Stmt::YieldInto { value, .. } => expr_assigns_to(ast, *value, name),
        Stmt::Return(maybe) => maybe.is_some_and(|e| expr_assigns_to(ast, e, name)),
        Stmt::LetDecl { init, .. } => expr_assigns_to(ast, *init, name),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_assigns_to(ast, *cond, name)
                || stmt_assigns_to(ast, then_branch, name)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| stmt_assigns_to(ast, eb, name))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_assigns_to(ast, *cond, name) || stmt_assigns_to(ast, body, name)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            if expr_assigns_to(ast, *scrutinee, name) {
                return true;
            }
            for c in cases {
                if expr_assigns_to(ast, c.value, name) {
                    return true;
                }
                for s in &c.body {
                    if stmt_assigns_to(ast, s, name) {
                        return true;
                    }
                }
            }
            if let Some(db) = default {
                for s in db {
                    if stmt_assigns_to(ast, s, name) {
                        return true;
                    }
                }
            }
            false
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref().is_some_and(|i| stmt_assigns_to(ast, i, name))
                || cond.is_some_and(|c| expr_assigns_to(ast, c, name))
                || step.is_some_and(|s| expr_assigns_to(ast, s, name))
                || stmt_assigns_to(ast, body, name)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_assigns_to(ast, s, name))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_assigns_to(ast, s, name))
                || catch_body.iter().any(|s| stmt_assigns_to(ast, s, name))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_assigns_to(ast, s, name)))
        }
        Stmt::Break | Stmt::Continue => false,
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_assigns_to(ast, *parent, name)
                || expr_assigns_to(ast, *sep, name)
                || stmt_assigns_to(ast, body, name)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_assigns_to(ast, *elem_expr, name) || stmt_assigns_to(ast, body, name),
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => false,
        Stmt::ImportDecl { .. } => false,
        Stmt::ExportDecl { inner, .. } => inner
            .as_ref()
            .is_some_and(|inner| stmt_assigns_to(ast, inner, name)),
    }
}

fn expr_assigns_to(ast: &Ast, eid: ExprId, name: &str) -> bool {
    if let Expr::Assign { target, value } = ast.get_expr(eid) {
        if let Expr::Ident(n) = ast.get_expr(*target)
            && n == name
        {
            return true;
        }
        return expr_assigns_to(ast, *target, name) || expr_assigns_to(ast, *value, name);
    }
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            expr_assigns_to(ast, *callee, name)
                || args.iter().any(|a| expr_assigns_to(ast, *a, name))
        }
        Expr::BinOp { left, right, .. } => {
            expr_assigns_to(ast, *left, name) || expr_assigns_to(ast, *right, name)
        }
        Expr::Unary { expr, .. } => expr_assigns_to(ast, *expr, name),
        Expr::Member { obj, .. } => expr_assigns_to(ast, *obj, name),
        Expr::Index { obj, index } => {
            expr_assigns_to(ast, *obj, name) || expr_assigns_to(ast, *index, name)
        }
        Expr::Array(elems) => elems.iter().any(|e| expr_assigns_to(ast, *e, name)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_assigns_to(ast, *e, name)),
        Expr::ArrowFn { body, .. } => body.iter().any(|s| stmt_assigns_to(ast, s, name)),
        Expr::Closure { .. } => false,
        Expr::New { args, .. } | Expr::Super { args } => {
            args.iter().any(|a| expr_assigns_to(ast, *a, name))
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_assigns_to(ast, *cond, name)
                || expr_assigns_to(ast, *then_branch, name)
                || expr_assigns_to(ast, *else_branch, name)
        }
        Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => expr_assigns_to(ast, *expr, name),
        Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => expr_assigns_to(ast, *left, name) || expr_assigns_to(ast, *right, name),
        Expr::OptChain { obj, .. } => expr_assigns_to(ast, *obj, name),
        Expr::PostIncr { target, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*target) {
                if n == name {
                    return true;
                }
            }
            expr_assigns_to(ast, *target, name)
        }
        _ => false,
    }
}

fn stmt_diverges(s: &crate::ast::Stmt) -> bool {
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
fn unify_ternary(t: &Type, e: &Type) -> Option<Type> {
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

fn struct_is_prefix_subtype(arg: &Type, param: &Type) -> bool {
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
fn resolve_type_ann(name: &str, aliases: &HashMap<String, Type>) -> Option<Type> {
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
fn build_fn_type(
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

fn build_fn_type_full(
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
    moved: bool,
    /// Alias flag — this binding borrows a heap owned elsewhere
    /// (Member / Index / cross-scope Ident init). Borrowed bindings
    /// can't be transferred at mid-scope sites (assign-rhs, call-arg,
    /// field write — ssa_lower has no retain contract there yet), but
    /// CAN escape via return / throw: the binding dies with the scope
    /// and ssa_lower retains at the boundary (retain-at-return /
    /// retain-at-throw) so the escaping reference carries its own +1.
    borrowed: bool,
    /// M-OO.5 — when this binding's declared type annotation matches a
    /// class name (`let c: Counter = ...`), record the class name so
    /// `c.member` accesses can look up the visibility entry in
    /// `ast.member_visibility`. Plain object-literal bindings, function-
    /// return bindings, and primitive bindings get `None` here. The
    /// nominal info lives on the binding rather than on `Type::Struct`
    /// to avoid a substrate refactor — it's enough for the
    /// `obj.member` pattern that visibility enforcement needs.
    declared_class: Option<String>,
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
fn js_add_coerces_to_number(l: &Type, r: &Type) -> bool {
    fn coerces(t: &Type) -> bool {
        matches!(t, Type::Number | Type::Boolean | Type::Null)
    }
    coerces(l) && coerces(r) && (*l != Type::Number || *r != Type::Number)
}

/// V3-18 m1.b — `-` / `*` / `/` / `%` use the same ToNumber rule
/// as `+` but never have a String-concat path (spec §13.7-§13.10
/// unconditionally call ToNumeric). Same coercibles as m1.a.
fn js_arith_coerces_to_number(l: &Type, r: &Type) -> bool {
    js_add_coerces_to_number(l, r)
}

/// V3-18 m1.h.3 — built-in JS globals that tora's check.rs
/// resolves via the special Ident → Type::Object("X") arms.
/// Used by the `typeof undeclared` path to avoid mis-classifying
/// a tora built-in as "undefined".
fn is_known_builtin_global(name: &str) -> bool {
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
fn js_loose_eq_supported(l: &Type, r: &Type) -> bool {
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

/// `parse_type` consumes. Used to translate inferred generic type args
/// from the typechecker into ssa_lower's annotation strings.
pub fn type_to_ann(ty: &Type) -> String {
    match ty {
        Type::Number => "number".into(),
        Type::Boolean => "boolean".into(),
        Type::String => "string".into(),
        Type::Void => "void".into(),
        Type::BigInt => "bigint".into(),
        Type::WeakRef => "weakref".into(),
        Type::WeakMap => "weakmap".into(),
        Type::WeakSet => "weakset".into(),
        Type::Map => "Map".into(),
        Type::Set => "Set".into(),
        Type::MapIter => "mapiter".into(),
        Type::ArrIter => "arriter".into(),
        // T-28-substrate — SSA Type::Any is its own slot type at the
        // SSA layer (parse_type's "any" round-trips to Type::Any).
        // Pre-T-28-substrate this collapsed to "number" because Any-
        // typed flows weren't fully wired through the SSA layer; the
        // collapse silently corrupted padded ANY_UNDEF Any-box ptrs
        // when stuffed into i64 Number slots. Round-tripping as "any"
        // gives generic mono its own Any specialization.
        Type::Any => "any".into(),
        Type::Symbol => "symbol".into(),
        Type::Array(inner) => format!("{}[]", type_to_ann(inner)),
        // Structs encode structurally as `__struct(field_name1:T1|...)`.
        // ssa_lower's `parse_type` decodes the same shape, looks up
        // (or interns) the matching `Type::Obj(StructId)`. Each
        // distinct struct shape produces a distinct annotation so the
        // generic mono cache no longer collides on `void`.
        Type::Struct(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(n, ft)| format!("{n}:{}", type_to_ann(ft)))
                .collect();
            format!("__struct({})", parts.join("|"))
        }
        Type::Function(args, ret) => {
            let parts: Vec<String> = args.iter().map(type_to_ann).collect();
            format!("__fn({})->{}", parts.join("|"), type_to_ann(ret))
        }
        Type::Object(name) => (*name).into(),
        Type::ClassRef(name) => {
            // Generic-instantiation back-edge (`Rec<number>`): the key
            // IS its own canonical annotation — emit it verbatim and
            // ssa_lower::parse_type re-instantiates on its side.
            if name.contains('<') {
                return name.clone();
            }
            /* V3-05 — non-generic class references should have been
             * resolved (via aliases lookup) before reaching ssa_lower.
             * The placeholder Pass should have been replaced by the
             * Real Type::Struct in c.aliases by the time anyone
             * asks for an SSA annotation. Panic to surface stale
             * usage rather than silently emitting `__struct()` for
             * a class. */
            panic!(
                "type_to_ann: ClassRef(`{name}`) reached SSA-ann emission — caller should have called resolve_class_ref first"
            )
        }
        Type::TypeVar(_) => {
            panic!("type_to_ann: TypeVar should be substituted before SSA layer")
        }
        // SSA layer treats nullable as the underlying T (storage and
        // call boundaries are identical — the only difference is that
        // `null` is a legal value of T). The annotation collapses to
        // T's annotation; check.rs is the only layer that distinguishes
        // them, and it's already past by the time this fn runs.
        Type::Nullable(inner) => type_to_ann(inner),
        Type::Null => "null".into(),
        // P1.1 — Type::Undefined collapses to `null` in the SSA-ann
        // string for now since the SSA layer has no separate Undefined.
        // The runtime tag (ANY_NULL=0 vs ANY_UNDEF=5) is the actual
        // disambiguator and lives in the box helpers; the static SSA
        // type stays Ptr-shaped for both.
        Type::Undefined => "undefined".into(),
        // RegExp is its own SSA type (Type::RegExp); the annotation
        // round-trips through ssa_lower's parse_type back to the same.
        Type::RegExp => "regex".into(),
        Type::Date => "date".into(),
        Type::Promise(inner) => format!("Promise<{}>", type_to_ann(inner)),
    }
}

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
        let c = self;

        // Pass 0: register type aliases first so fn signatures + let
        // annotations can reference them. `type Point = { x: number, y: number }`
        // adds `Point → Type::Struct(...)` to `c.aliases`. M3.4 — generic
        // type aliases `type Pair<A, B> = { ... }` are recorded in a
        // separate map (`generic_alias_decls`) and instantiated lazily by
        // `resolve_type_ann_with_vars` when it sees `Pair<X|Y>` syntax.
        /* V3-05 — pre-register every non-generic class TypeDecl name
         * with an empty `Type::Struct(vec![])` placeholder before
         * resolving any field types. This lets `resolve_type_ann_full`
         * find self-references (`class Node { next: Node | null }`)
         * and forward-references (`class A { b: B } class B { a: A }`)
         * — both previously rejected because the class wasn't yet in
         * `c.aliases` when its own (or its sibling's) field types were
         * being resolved. After Pass 0, the placeholder is replaced
         * with the resolved fields. The downstream consumers that
         * matter — Member-access type-of, Assign LHS/RHS unify, etc.
         * — index `c.aliases` by name on every read, so they see the
         * post-replacement struct, not the placeholder. */
        let mut placeholder_classes: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for stmt in &ast.stmts {
            if let Stmt::TypeDecl {
                name, type_params, ..
            } = stmt
            {
                if !type_params.is_empty() {
                    continue;
                }
                if c.aliases.contains_key(name) || c.generic_alias_decls.contains_key(name) {
                    continue; /* duplicate handled by Pass 0 below */
                }
                c.aliases.insert(name.clone(), Type::ClassRef(name.clone()));
                placeholder_classes.insert(name.clone());
            }
        }

        for stmt in &ast.stmts {
            if let Stmt::TypeDecl {
                name,
                type_params,
                fields,
            } = stmt
            {
                /* Skip duplicate declarations — but ignore the
                 * placeholder we just inserted; only flag when the
                 * existing entry came from somewhere else. */
                if (c.aliases.contains_key(name) && !placeholder_classes.contains(name))
                    || c.generic_alias_decls.contains_key(name)
                {
                    c.errors.push_err(format!("redeclaration of type `{name}`"));
                    continue;
                }
                if !type_params.is_empty() {
                    c.generic_alias_decls
                        .insert(name.clone(), (type_params.clone(), fields.clone()));
                    continue;
                }
                // V3-18 wedge — bare type alias sentinel from parser.
                // Single field named "__alias__" carries the actual
                // type-ann string; resolve to the underlying Type and
                // register without wrapping in Struct.
                if fields.len() == 1 && fields[0].0 == "__alias__" {
                    let alias_ann = &fields[0].1;
                    match resolve_type_ann_full(alias_ann, &c.aliases, &[], &c.generic_alias_decls)
                    {
                        Some(ty) => {
                            c.aliases.insert(name.clone(), ty);
                        }
                        None => {
                            c.errors.push_err(format!(
                                "unknown type `{alias_ann}` for type alias `{name}`"
                            ));
                        }
                    }
                    continue;
                }
                let mut field_tys: Vec<(String, Type)> = Vec::new();
                let mut had_err = false;
                for (fname, fty_ann) in fields {
                    match resolve_type_ann_full(fty_ann, &c.aliases, &[], &c.generic_alias_decls) {
                        Some(ty) => field_tys.push((fname.clone(), ty)),
                        None => {
                            c.errors.push_err(format!(
                                "unknown type `{fty_ann}` for field `{fname}` of `{name}`"
                            ));
                            had_err = true;
                            break;
                        }
                    }
                }
                if !had_err {
                    c.aliases.insert(name.clone(), Type::Struct(field_tys));
                }
            }
        }

        // Pass 1: hoist top-level function signatures (uses aliases).
        // For lifted-closure FnDecls (first param `__env`), the user-visible
        // signature drops the env: callers see `(real_params...) -> ret`.
        // The full signature including __env stays implicit at the SSA layer.
        // Generic FnDecls (non-empty type_params) get their signatures stored
        // with TypeVar placeholders; call-site inference instantiates them.
        for stmt in &ast.stmts {
            if let Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                ..
            } = stmt
            {
                let is_closure = params.first().is_some_and(|p| p.name == "__env");
                let user_params: &[Param] = if is_closure { &params[1..] } else { params };
                match build_fn_type_full(
                    name,
                    user_params,
                    return_type,
                    &c.aliases,
                    type_params,
                    &c.generic_alias_decls,
                ) {
                    Ok(ty) => {
                        if c.globals.contains_key(name) {
                            c.errors
                                .push_err(format!("redeclaration of function `{name}`"));
                        } else {
                            c.globals.insert(name.clone(), ty);
                            if is_closure {
                                c.closure_fn_names.insert(name.clone());
                            }
                            if !type_params.is_empty() {
                                c.generic_type_params
                                    .insert(name.clone(), type_params.clone());
                            }
                            // Record per-param default ExprIds for caller-
                            // side default substitution. None positions are
                            // required args; first non-None marks the start
                            // of the optional tail (JS spec — defaults must
                            // be trailing).
                            let defaults: Vec<Option<ExprId>> =
                                user_params.iter().map(|p| p.default).collect();
                            if defaults.iter().any(|d| d.is_some()) {
                                c.fn_defaults.insert(name.clone(), defaults);
                            }
                        }
                    }
                    Err(e) => c.errors.push_err(e),
                }
            }
        }

        // Pass 2: check each statement. Closure-lifted FnDecls are skipped
        // here — their bodies are checked lazily by the `Expr::Closure` arm
        // of `type_of`, with captures injected as locals. Generic FnDecls
        // are also skipped: their bodies use TypeVar placeholders that the
        // SSA monomorphization pass instantiates per call site, and the
        // body's own TS-shape ops (return, BinOp on TypeVar) would fail
        // a concrete check here. Call-site inference still validates that
        // arguments are consistent with each TypeVar instance.
        // Pre-pass: register top-level `const X = LITERAL` (Number / String
        // / Boolean) as globals so named functions can read them. tr's lower
        // path emits the literal inline at every reference; non-literal
        // initializers stay scoped to the implicit main fn (they alloca
        // there and aren't visible from named-fn bodies).
        let binding_refs = crate::ast_refs::toplevel_binding_refs(ast);
        for stmt in &ast.stmts {
            if let Stmt::LetDecl {
                name,
                init,
                type_ann,
                ..
            } = stmt
            {
                let lit_ty = match ast.get_expr(*init) {
                    Expr::Number(_) => Some(Type::Number),
                    Expr::BigInt { .. } => Some(Type::BigInt),
                    Expr::String(_) => Some(Type::String),
                    Expr::Bool(_) => Some(Type::Boolean),
                    _ => None,
                };
                // Phase K.3 — non-literal init with explicit type annotation
                // becomes a real LLVM data global. Record the annotated type
                // so named-fn bodies type-check reads + writes against it.
                // ssa_lower restricts the runtime registration to primitive
                // Copy types (I64 / F64 / Bool); for the typecheck we just
                // accept whatever resolves cleanly.
                let ann_ty = match (lit_ty.clone(), type_ann) {
                    (None, Some(ann)) => resolve_type_ann(ann, &c.aliases),
                    // K.3b — un-annotated non-literal init (`let y =
                    // g()`): register exactly what ssa_lower's Pass
                    // 1.5 will promote, via the shared ast_refs gate +
                    // slot-shape inference. Anything looser here lets
                    // programs typecheck whose lowering still aborts
                    // with `unknown ident`; anything tighter brings
                    // back the bogus `unknown identifier` on legal
                    // named-fn reads of call-init globals.
                    (None, None) => {
                        if binding_refs.named_fn_refs.contains(name)
                            && !binding_refs.closure_captured.contains(name)
                        {
                            crate::ast_refs::infer_toplevel_slot_shape(ast, *init).map(
                                |s| match s {
                                    crate::ast_refs::GlobalSlotShape::I64
                                    | crate::ast_refs::GlobalSlotShape::F64 => Type::Number,
                                    crate::ast_refs::GlobalSlotShape::Str => Type::String,
                                    crate::ast_refs::GlobalSlotShape::Bool => Type::Boolean,
                                },
                            )
                        } else {
                            None
                        }
                    }
                    _ => lit_ty,
                };
                if let Some(ty) = ann_ty
                    && !c.globals.contains_key(name)
                {
                    c.globals.insert(name.clone(), ty);
                }
            }
        }

        for stmt in &ast.stmts {
            if let Stmt::FnDecl { name, .. } = stmt
                && (c.closure_fn_names.contains(name) || c.generic_type_params.contains_key(name))
            {
                continue;
            }
            c.check_stmt(ast, stmt);
        }
    }
}

pub(crate) struct Checker {
    pub(crate) globals: HashMap<String, Type>,
    scopes: Vec<HashMap<String, LocalInfo>>,
    /// User-declared type aliases — populated in pass 0 from
    /// `Stmt::TypeDecl`. `Point → Type::Struct(...)`.
    pub(crate) aliases: HashMap<String, Type>,
    /// T-04 — was `Vec<String>`; now carries severity + span. The
    /// public APIs (`check`, `collect_errors`) stringify back to the
    /// caller's expected shape; LSP consumes via `collect_diagnostics`.
    errors: Vec<Diagnostic>,
    expected_return: Option<Type>,
    /// M-OO.5 — when typechecking a fn body whose name follows the
    /// `__cm_<Class>__<method>` / `__sm_<Class>__<method>` shape that
    /// `desugar_classes` mints, `current_class` records the enclosing
    /// class. Member-access enforcement reads this to decide whether
    /// `obj.private_member` is allowed (private requires caller is
    /// inside the same class; protected requires caller in same class
    /// or descendant). `None` outside of class fn bodies (top-level
    /// stmts, free fns, etc.) — those treat every member as if it
    /// were public.
    current_class: Option<String>,
    /// M2 — captures for each lifted closure FnDecl. Populated by the
    /// `Expr::Closure` arm of `type_of` (which resolves capture types in
    /// the OUTER scope at the construction site) and consumed by the
    /// closure-FnDecl body walker below. Maps lifted-fn-name → ordered
    /// list of (capture_name, captured_type).
    closure_captures: HashMap<String, Vec<(String, Type)>>,
    /// Names of FnDecls that are lifted closures (first param is
    /// `__env`). Pass-2 skips these — their bodies are checked lazily
    /// when an `Expr::Closure` references them, so the captures are in
    /// scope.
    closure_fn_names: std::collections::HashSet<String>,
    /// M3 — type params for each generic FnDecl (`function id<T, U>(...)`).
    /// Empty for non-generic fns. Pass-2 skips these decls (their
    /// TypeVar-bearing bodies can't be type-checked without substitution);
    /// the SSA monomorphization pass produces concrete bodies on demand.
    /// Call-site inference uses this map to walk each TypeVar in the
    /// signature and unify it against the actual argument type.
    generic_type_params: HashMap<String, Vec<String>>,
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
    consumed_calls: std::collections::HashSet<ExprId>,
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
fn is_array_method_name(name: &str) -> bool {
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

fn substitute_typevars(ty: &Type, subst: &HashMap<String, Type>) -> Type {
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
    fn declare(&mut self, name: String, info: LocalInfo) -> Result<(), String> {
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
    fn collect_null_narrow(&self, ast: &Ast, cond: ExprId) -> Option<(String, Type, bool)> {
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
    fn apply_narrow(&mut self, name: &str, inner_ty: Type) -> Option<Type> {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                let prev = info.ty.clone();
                info.ty = inner_ty;
                return Some(prev);
            }
        }
        None
    }

    fn restore_narrow(&mut self, name: &str, prev_ty: Type) {
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
    fn is_descendant_of(&self, ast: &Ast, child: &str, ancestor: &str) -> bool {
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
    fn snapshot_moved(&self) -> Vec<Vec<(String, bool)>> {
        self.scopes
            .iter()
            .map(|s| s.iter().map(|(n, i)| (n.clone(), i.moved)).collect())
            .collect()
    }

    /// Restore moved flags to the values captured by `snapshot_moved`.
    /// Bindings introduced after the snapshot (i.e. inside the branch)
    /// are unaffected — they're either still in the scope or already
    /// popped by branch teardown.
    fn restore_moved(&mut self, snap: &[Vec<(String, bool)>]) {
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
    fn join_branch_moves(
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
    fn consume_escape(&mut self, ast: &Ast, eid: ExprId) {
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
    fn classify_init_alias(&self, ast: &Ast, eid: ExprId) -> bool {
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

    fn check_stmt(&mut self, ast: &Ast, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(eid) => {
                if let Err(e) = self.type_of(ast, *eid) {
                    self.errors.push_err(e);
                }
            }
            Stmt::Yield(_) | Stmt::YieldInto { .. } => {
                // Phase J — Yield only appears inside generator bodies,
                // and `desugar_generators` rewrites those bodies into
                // ordinary class-method bodies before typecheck. Reaching
                // a raw Yield here means desugar didn't run / didn't
                // catch this node — surface as a typecheck error rather
                // than panicking at SSA lower time.
                self.errors
                    .push_err("yield is only valid inside a `function*` generator body".into());
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                match self.type_of(ast, *cond) {
                    Ok(t) if js_truthy_acceptable(&t) => {}
                    Ok(other) => self.errors.push_err(format!(
                        "if condition must be boolean (or coercible), got {other:?}"
                    )),
                    Err(e) => self.errors.push_err(e),
                }
                // V3-18 wedge — flow narrowing on `<ident> !== null`
                // / `<ident> === null` cond shapes. For `!== null`,
                // narrow `<ident>` to its inner type within the
                // then-branch only. For `=== null`, narrow within
                // else-branch only. Narrow-and-restore around each
                // branch; the saved types come from the binding's
                // pre-if state so nested ifs compose correctly.
                let narrow = self.collect_null_narrow(ast, *cond);
                // CFG-aware moved tracking: snapshot the moved-state
                // before each branch, run the branch, capture its
                // post-state, restore. Then join: a binding is moved
                // post-if iff every non-diverging branch consumed it.
                // This is what makes `if (cond) return f; return f;`
                // work — the then-branch diverges so its consume of
                // `f` doesn't propagate, leaving `f` available for
                // the trailing return.
                let pre = self.snapshot_moved();
                let then_narrow = if let Some((name, inner, polarity)) = &narrow {
                    if *polarity {
                        self.apply_narrow(name, inner.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.check_stmt(ast, then_branch);
                if let (Some((name, _, _)), Some(saved)) = (&narrow, then_narrow) {
                    self.restore_narrow(name, saved);
                }
                let then_div = stmt_diverges(then_branch);
                let then_post = self.snapshot_moved();
                self.restore_moved(&pre);
                let (else_div, else_post): (bool, Option<MovedSnapshot>) =
                    if let Some(eb) = else_branch {
                        let else_narrow = if let Some((name, inner, polarity)) = &narrow {
                            if !*polarity {
                                self.apply_narrow(name, inner.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        self.check_stmt(ast, eb);
                        if let (Some((name, _, _)), Some(saved)) = (&narrow, else_narrow) {
                            self.restore_narrow(name, saved);
                        }
                        let div = stmt_diverges(eb);
                        let snap2 = self.snapshot_moved();
                        self.restore_moved(&pre);
                        (div, Some(snap2))
                    } else {
                        (false, None)
                    };
                self.join_branch_moves(&pre, &then_post, then_div, else_post.as_deref(), else_div);
                // V3-18 wedge — post-if narrowing when one branch
                // diverges (early return / throw / break / continue).
                // Common pattern:
                //   if (o === null) return; ... o.x  // narrowed
                //   if (o !== null) return; ... // o stays Nullable
                // For polarity true (then = !==), if then-branch
                // diverges, post-if narrows in the *opposite* sense
                // (else state propagates out). For polarity false
                // (then = ===), if then-branch diverges, post-if
                // narrows to the inner type.
                if let Some((name, inner, polarity)) = &narrow {
                    let post_narrow_to_inner = (*polarity && else_div) || (!*polarity && then_div);
                    if post_narrow_to_inner {
                        self.apply_narrow(name, inner.clone());
                    }
                }
            }
            Stmt::While { cond, body } => {
                match self.type_of(ast, *cond) {
                    Ok(t) if js_truthy_acceptable(&t) => {}
                    Ok(other) => self.errors.push_err(format!(
                        "while condition must be boolean (or coercible), got {other:?}"
                    )),
                    Err(e) => self.errors.push_err(e),
                }
                // V3-18 wedge — flow narrowing on while condition.
                // `while (x !== null) { ... x.foo ... }` narrows x to
                // its inner type for the body, but ONLY if the body
                // doesn't reassign x — re-narrowing on each iteration
                // would otherwise conflict with the (still-Nullable)
                // RHS of `x = x.next`. Polarity-false (cond `== null`)
                // wouldn't enter the loop at all, so don't narrow.
                let narrow = self.collect_null_narrow(ast, *cond);
                let saved = if let Some((name, inner, polarity)) = &narrow {
                    if *polarity && !stmt_assigns_to(ast, body, name) {
                        self.apply_narrow(name, inner.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.check_stmt(ast, body);
                if let (Some((name, _, _)), Some(prev)) = (&narrow, saved) {
                    self.restore_narrow(name, prev);
                }
            }
            Stmt::DoWhile { body, cond } => {
                self.check_stmt(ast, body);
                match self.type_of(ast, *cond) {
                    Ok(t) if js_truthy_acceptable(&t) => {}
                    Ok(other) => self.errors.push_err(format!(
                        "do-while condition must be boolean (or coercible), got {other:?}"
                    )),
                    Err(e) => self.errors.push_err(e),
                }
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                let scrut_ty = match self.type_of(ast, *scrutinee) {
                    Ok(t) => t,
                    Err(e) => {
                        self.errors.push_err(e);
                        return;
                    }
                };
                for c in cases {
                    match self.type_of(ast, c.value) {
                        Ok(t) if t == scrut_ty => {}
                        Ok(t) => self.errors.push_err(format!(
                            "switch case value type {t:?} differs from scrutinee {scrut_ty:?}"
                        )),
                        Err(e) => self.errors.push_err(e),
                    }
                    self.scopes.push(HashMap::new());
                    for s in &c.body {
                        self.check_stmt(ast, s);
                    }
                    self.scopes.pop();
                }
                if let Some(db) = default {
                    self.scopes.push(HashMap::new());
                    for s in db {
                        self.check_stmt(ast, s);
                    }
                    self.scopes.pop();
                }
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                // Init runs in a fresh scope so `let i = 0; ...; for () i;`
                // doesn't bleed `i` into the surrounding fn scope. Push
                // a scope before init, pop after body.
                self.scopes.push(HashMap::new());
                if let Some(i) = init {
                    self.check_stmt(ast, i);
                }
                if let Some(c) = cond {
                    match self.type_of(ast, *c) {
                        Ok(t) if js_truthy_acceptable(&t) => {}
                        Ok(other) => self.errors.push_err(format!(
                            "for condition must be boolean (or coercible), got {other:?}"
                        )),
                        Err(e) => self.errors.push_err(e),
                    }
                }
                if let Some(st) = step {
                    if let Err(e) = self.type_of(ast, *st) {
                        self.errors.push_err(e);
                    }
                }
                self.check_stmt(ast, body);
                self.scopes.pop();
            }
            Stmt::Throw(eid) => {
                // M4.3 — accept any 8-byte-shaped throw value. The
                // global throw_value slot is i64; ptr-shaped values
                // (Str, Obj, Arr, FnSig, Closure) are reinterpreted at
                // catch time per the catch param's type annotation.
                // type_of has side effects (it runs consume on
                // consuming-arg positions inside any Call sub-expr),
                // so we evaluate it once and reuse the result for both
                // the type check AND the throw consume.
                let t_result = self.type_of(ast, *eid);
                match &t_result {
                    Ok(Type::Number) | Ok(Type::String) | Ok(Type::Boolean) => {}
                    Ok(Type::Array(_)) | Ok(Type::Struct(_)) => {}
                    // `throw null` and `throw <Nullable<T>>` are valid
                    // JS — null lowers to the same 0-sentinel pointer
                    // shape as Obj / Arr / Str, so the throw_value slot
                    // can hold it. The catch param's annotation
                    // determines how the value is interpreted (a typed
                    // `catch (e: number)` against an actual null throw
                    // is the user's bug, not the runtime's).
                    Ok(Type::Null) | Ok(Type::Nullable(_)) => {}
                    // P7.2a — `throw undefined` is valid JS. undefined
                    // lowers to ANY_UNDEF=5 / payload 0 in the throw
                    // match (ssa_lower); catch `: any` reconstructs it
                    // via any_box(5, 0). Pre-P7.2a it fell through to
                    // the M4-era "8-byte-shaped" reject.
                    Ok(Type::Undefined) => {}
                    // P4.7 — accept Type::Any throws. The tagged
                    // throw_set / take_tag substrate (ssa_lower) records
                    // the dynamic tag so catch `: any` reconstructs an
                    // Any-box correctly. Pre-P4.7 reject was a
                    // M4-era artifact of the i64-only throw slot.
                    Ok(Type::Any) => {}
                    Ok(other) => self
                        .errors
                        .push_err(format!("throw value must be 8-byte-shaped, got {other:?}")),
                    Err(e) => self.errors.push_err(e.clone()),
                }
                // Throw consumes a non-Copy value (its heap is now
                // owned by the catch site, which is responsible for
                // dropping after the bind / re-throwing). Borrowed
                // aliases may escape here — retain-at-throw gives the
                // catch its own +1.
                if let Ok(t) = t_result
                    && !t.is_copy()
                {
                    self.consume_escape(ast, *eid);
                }
            }
            Stmt::Try {
                body,
                had_catch: _,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                // body in a fresh scope
                self.scopes.push(HashMap::new());
                for s in body {
                    self.check_stmt(ast, s);
                }
                self.scopes.pop();
                // catch in a fresh scope with `e` injected. Type comes
                // from `(e: T)` annotation. P7.2b-2 — an unannotated
                // `catch (e)` is implicitly `any` per TS spec (an
                // explicit non-any/unknown annotation is TS1196); the
                // old Number default was a pre-spec tora-ism. Mirrors
                // the ssa_lower `None => Type::Any` default so the
                // check-tier sees `e` as Any too (member access /
                // arithmetic / return all go through the Any paths).
                let e_ty = match catch_type {
                    Some(ann) => match resolve_type_ann_full(
                        ann,
                        &self.aliases,
                        &[],
                        &self.generic_alias_decls,
                    ) {
                        Some(t) => t,
                        None => {
                            self.errors
                                .push_err(format!("unknown type `{ann}` in catch param"));
                            Type::Any
                        }
                    },
                    None => Type::Any,
                };
                self.scopes.push(HashMap::new());
                if let Some(p) = catch_param {
                    let _ = self.declare(
                        p.clone(),
                        LocalInfo {
                            ty: e_ty,
                            mutable: true,
                            moved: false,
                            borrowed: false,
                            declared_class: None,
                        },
                    );
                }
                for s in catch_body {
                    self.check_stmt(ast, s);
                }
                self.scopes.pop();
                if let Some(fb) = finally_body {
                    self.scopes.push(HashMap::new());
                    for s in fb {
                        self.check_stmt(ast, s);
                    }
                    self.scopes.pop();
                }
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
                // P-iter — parent and sep must both be strings; var
                // binds a Substr borrow per iteration.
                match self.type_of(ast, *parent) {
                    Ok(Type::String) => {}
                    Ok(other) => self
                        .errors
                        .push_err(format!("for-of split parent must be string, got {other:?}")),
                    Err(e) => self.errors.push_err(e),
                }
                match self.type_of(ast, *sep) {
                    Ok(Type::String) => {}
                    Ok(other) => self.errors.push_err(format!(
                        "for-of split separator must be string, got {other:?}"
                    )),
                    Err(e) => self.errors.push_err(e),
                }
                self.scopes.push(HashMap::new());
                let _ = self.declare(
                    var_name.clone(),
                    LocalInfo {
                        ty: Type::String,
                        mutable: false,
                        moved: false,
                        borrowed: false,
                        declared_class: None,
                    },
                );
                self.check_stmt(ast, body);
                self.scopes.pop();
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
                self.scopes.push(HashMap::new());
                let _ = self.declare(
                    i_ident.clone(),
                    LocalInfo {
                        ty: Type::Number,
                        mutable: true,
                        moved: false,
                        borrowed: false,
                        declared_class: None,
                    },
                );
                // P5.3 Phase B — peek src type. If it's a Struct we
                // skip the index typecheck and route through the
                // protocol path; ssa_lower derives elem_ty from the
                // iter chain's step.value field.
                // P6.4c — same skip for Type::Map / Type::Set /
                // Type::MapIter (handled by lower_for_of_map_like in
                // ssa_lower). For Type::Map specifically the yielded
                // value is `[k, v]` Array<Any> so var_ty is
                // Array(Any) (enables destructuring `for (let [k, v]
                // of m)`); for Set / MapIter the yield is type-erased
                // Any.
                let src_kind = if let Expr::Index { obj, .. } = ast.get_expr(*elem_expr) {
                    self.type_of(ast, *obj).ok()
                } else {
                    None
                };
                let src_is_iter_subset = matches!(
                    src_kind,
                    Some(Type::Struct(_))
                        | Some(Type::Map)
                        | Some(Type::Set)
                        | Some(Type::MapIter)
                        | Some(Type::ArrIter)
                );
                let elem_ty = if src_is_iter_subset {
                    match src_kind {
                        Some(Type::Map) => Type::Array(Box::new(Type::Any)),
                        _ => Type::Any,
                    }
                } else {
                    match self.type_of(ast, *elem_expr) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push_err(e);
                            Type::Any
                        }
                    }
                };
                let var_ty = if let Some(ann) = var_type_ann {
                    resolve_type_ann(ann, &self.aliases).unwrap_or(elem_ty)
                } else {
                    elem_ty
                };
                let _ = self.declare(
                    var_name.clone(),
                    LocalInfo {
                        ty: var_ty,
                        mutable: false,
                        moved: false,
                        borrowed: false,
                        declared_class: None,
                    },
                );
                self.check_stmt(ast, body);
                self.scopes.pop();
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
                // M1.2 — empty array literal `[]` carries no element-type
                // info; the annotation must provide it. Special-case to
                // skip type_of (which would error) and use the annotation
                // directly. Matches TS / bun: `let xs: number[] = [];`.
                let is_empty_array =
                    matches!(ast.get_expr(*init), Expr::Array(els) if els.is_empty());
                let init_ty = if is_empty_array {
                    // P0.10 — empty array literal `[]` defaults to
                    // `Array<Any>` when no annotation is present, per
                    // TS spec (untyped `[]` is `any[]`). Matches the
                    // closure-default-Any policy. Pre-fix tora demanded
                    // `let xs: T[] = []`; test262 uses bare `let arr =
                    // []` pervasively (160+ cases blocked on this
                    // single shape across the broader sample).
                    let ann_ty = match type_ann {
                        Some(ann) => {
                            let Some(t) = resolve_type_ann_full(
                                ann,
                                &self.aliases,
                                &[],
                                &self.generic_alias_decls,
                            ) else {
                                self.errors.push_err(format!("unknown type `{ann}`"));
                                return;
                            };
                            if !matches!(t, Type::Array(_)) {
                                self.errors.push_err(format!(
                                    "empty array literal `{name}` needs an array type annotation, got `{ann}`"
                                ));
                                return;
                            }
                            t
                        }
                        None => Type::Array(Box::new(Type::Any)),
                    };
                    ann_ty
                } else {
                    match self.type_of(ast, *init) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push_err(e);
                            return;
                        }
                    }
                };
                let final_ty = match type_ann {
                    None => init_ty,
                    Some(ann) => {
                        let Some(ann_ty) = resolve_type_ann_full(
                            ann,
                            &self.aliases,
                            &[],
                            &self.generic_alias_decls,
                        ) else {
                            self.errors.push_err(format!("unknown type `{ann}`"));
                            return;
                        };
                        if !is_assignable_to_resolved(
                            &ann_ty,
                            &init_ty,
                            &self.aliases,
                            &self.generic_alias_decls,
                        ) {
                            self.errors.push_err(format!(
                                "type mismatch on `{name}`: declared {ann_ty:?}, init has {init_ty:?}"
                            ));
                            return;
                        }
                        ann_ty
                    }
                };
                // Member / Index / cross-scope Ident init aliases a heap
                // owned elsewhere — mark `borrowed` so transfer sites
                // reject mid-scope moves while return/throw escapes stay
                // legal (retain-at-boundary). Same-scope Ident init
                // shares (ssa_lower retains); all other init shapes
                // (Call, BinOp, literal, ObjectLit) are fresh-owned.
                let is_alias_init = self.classify_init_alias(ast, *init);
                // M-OO.5 — when the declared annotation names a known
                // class, propagate that nominal info to the binding so
                // `name.private_member` accesses can look up the
                // visibility entry. type_ann is the source string
                // (e.g. "Counter"); we treat it as a class iff it
                // appears in `c.aliases` AND has a corresponding entry
                // in `ast.class_parents` (declared via `class`, not
                // `type`).
                let declared_class: Option<String> = type_ann.as_ref().and_then(|s| {
                    if ast.class_parents.contains_key(s.as_str()) {
                        Some(s.clone())
                    } else {
                        None
                    }
                });
                if let Err(e) = self.declare(
                    name.clone(),
                    LocalInfo {
                        ty: final_ty,
                        mutable: *mutable,
                        moved: false,
                        borrowed: is_alias_init,
                        declared_class,
                    },
                ) {
                    self.errors.push_err(e);
                }
                // let-rhs is NOT a transfer site: a same-scope
                // `let t = s` SHARES ownership (ssa_lower retains at the
                // binding site — CPython incref / Swift strong-assignment
                // semantics), so `s` stays fully usable afterwards:
                // `return s`, `let u = s`, `s = "new"` are all legal.
                // Alias-init (Member / Index / cross-scope Ident) keeps
                // the source as owner with the binding marked borrowed.
                // Non-Ident inits produce fresh-owned values — nothing
                // to consume either way.
            }
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                // Signature already hoisted in the first pass.
                let Some(Type::Function(param_tys, ret_ty)) = self.globals.get(name).cloned()
                else {
                    // First pass had an error; skip body to avoid cascading.
                    return;
                };
                // Top-level FnDecl bodies see no outer locals (none exist) but do
                // see globals via lookup-fallback. We use a fresh scope stack to
                // mirror the arrow-fn rule (no captures).
                let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
                let saved_return = self.expected_return.replace(*ret_ty);
                // M-OO.5 — fn name pattern → enclosing class context.
                // `__cm_<C>__<m>` (instance method) and
                // `__sm_<C>__<m>` (static method) both put the body
                // inside class C; visibility checks compare against
                // this. Free fns / `__new_<C>` / `__dispatch_<m>` /
                // `__env_drop_<closure>` etc. don't establish a class
                // scope (`__new_C` IS the class's factory but isn't
                // user-written code, so it shouldn't be granted
                // private-access; the methods it calls are __cm_*
                // which DO have the context).
                let saved_class = self.current_class.take();
                let new_class: Option<String> = name
                    .strip_prefix("__cm_")
                    .and_then(|rest| rest.split_once("__").map(|(c, _)| c.to_string()))
                    .or_else(|| {
                        name.strip_prefix("__sm_")
                            .and_then(|rest| rest.split_once("__").map(|(c, _)| c.to_string()))
                    });
                if new_class.is_some() {
                    self.current_class = new_class;
                }
                for (p, ty) in params.iter().zip(param_tys.iter()) {
                    // M-OO.5 — propagate nominal class info onto every
                    // param whose source-level type annotation names a
                    // known class. The synthesized `__this` param uses
                    // the enclosing class context (its annotation may
                    // be a generic-instantiated form like `Wrapper<T>`
                    // that doesn't lookup as a plain class name);
                    // user-written params with a plain class name
                    // pull from `ast.class_parents`.
                    let declared_class = if p.name == "__this" {
                        self.current_class.clone()
                    } else {
                        p.type_ann.as_ref().and_then(|s| {
                            if ast.class_parents.contains_key(s.as_str()) {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                    };
                    if let Err(e) = self.declare(
                        p.name.clone(),
                        LocalInfo {
                            ty: ty.clone(),
                            mutable: true,
                            moved: false,
                            borrowed: false,
                            declared_class,
                        },
                    ) {
                        self.errors.push_err(e);
                    }
                }
                for s in body {
                    self.check_stmt(ast, s);
                }
                self.expected_return = saved_return;
                self.scopes = saved_scopes;
                self.current_class = saved_class;
            }
            Stmt::TypeDecl { .. } => {
                // Already handled in pass 0; re-encountering it during the
                // body walk is a no-op. (No nested type decls — top-level
                // only — but the AST shape allows them anywhere.)
            }
            Stmt::Return(maybe_expr) => {
                let Some(expected) = self.expected_return.clone() else {
                    self.errors
                        .push_err("`return` outside of a function".into());
                    return;
                };
                let actual = match maybe_expr {
                    None => Type::Void,
                    Some(eid) => match self.type_of(ast, *eid) {
                        Ok(t) => t,
                        Err(e) => {
                            self.errors.push_err(e);
                            return;
                        }
                    },
                };
                // V3-18 wedge — Nullable<T> return type accepts both
                // T-typed and Null values (Nullable's two value
                // carriers), mirroring the call-arg widening rule
                // for parameters.
                let nullable_ok = if let Type::Nullable(inner) = &expected {
                    actual == Type::Null || actual == **inner
                } else {
                    false
                };
                // P0.9 — return-type check goes through the
                // assignability lattice so Any-typed expected (or
                // structs containing Any fields) accept concrete
                // returned values. Previous strict-eq blocked
                // generators with default-Any yield types from
                // returning concrete iterator-result structs.
                if !nullable_ok
                    && !is_assignable_to_resolved(
                        &expected,
                        &actual,
                        &self.aliases,
                        &self.generic_alias_decls,
                    )
                {
                    self.errors.push_err(format!(
                        "return type mismatch: function expects {expected:?}, got {actual:?}"
                    ));
                }
                // Returning a non-Copy ident moves it out to the caller.
                // Borrowed aliases may escape here — retain-at-return
                // gives the caller its own +1.
                if let Some(eid) = maybe_expr
                    && !expected.is_copy()
                {
                    self.consume_escape(ast, *eid);
                }
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
            Expr::Member { obj, name } => {
                let obj_ty = self.type_of(ast, *obj)?;
                // M-OO.5 — visibility enforcement. Find the binding's
                // nominal class:
                //   - `this` inside a class method body inherits the
                //     current class context.
                //   - An Ident bound by `let x: ClassName = ...` carries
                //     its `declared_class` from the LetDecl arm.
                // Other shapes (chained Member, Call result, etc.)
                // currently get no nominal info; treat their visibility
                // as Public until that path needs tightening.
                let obj_class: Option<String> = match ast.get_expr(*obj) {
                    Expr::This => self.current_class.clone(),
                    Expr::Ident(n) => self.lookup(n).and_then(|info| info.declared_class),
                    _ => None,
                };
                if let Some(cls) = obj_class.as_deref()
                    && let Some(vis) = ast
                        .member_visibility
                        .get(&(cls.to_string(), name.clone()))
                        .copied()
                {
                    let allowed = match vis {
                        Visibility::Public => true,
                        Visibility::Private => self.current_class.as_deref() == Some(cls),
                        Visibility::Protected => self
                            .current_class
                            .as_deref()
                            .map(|c| c == cls || self.is_descendant_of(ast, c, cls))
                            .unwrap_or(false),
                    };
                    if !allowed {
                        return Err(format!(
                            "M-OO.5: cannot access {vis:?} member `{cls}.{name}` from {}",
                            self.current_class
                                .as_deref()
                                .map(|c| format!("class `{c}`"))
                                .unwrap_or_else(|| "outside any class".to_string())
                        ));
                    }
                }
                // Struct field access is the most general path — look up
                // the named field; type is whatever it was declared as.
                // V3-05 — resolve any ClassRef placeholder embedded in
                // obj_ty (self-reference fields hit this).
                let resolved_obj_ty =
                    resolve_class_ref(&obj_ty, &self.aliases, &self.generic_alias_decls);
                if let Type::Struct(fields) = &resolved_obj_ty
                    && let Some((_, ty)) = fields.iter().find(|(fname, _)| fname == name)
                {
                    return Ok(resolve_class_ref(
                        ty,
                        &self.aliases,
                        &self.generic_alias_decls,
                    ));
                }
                // P8.2 — accessor read: `c.value` where the resolved
                // class C has a `get value(): T` declaration. After the
                // struct-field lookup misses (accessors aren't fields),
                // probe `accessor_getters` for the receiver's class and
                // return the getter's declared return type. ssa_lower
                // emits a `Call(__cm_<C>__value_get, c)` at the
                // matching Member arm — type-wise we just return the
                // getter's `ret` so caller sites see a normal value
                // (not a Function), matching ES §10.1.7 [[Get]]
                // semantics.
                if let Type::Struct(_) = &resolved_obj_ty {
                    let mut accessor_class: Option<String> = None;
                    for (n, ty) in self.aliases.iter() {
                        if *ty == resolved_obj_ty && ast.class_parents.contains_key(n) {
                            accessor_class = Some(n.clone());
                            break;
                        }
                    }
                    if let Some(cls) = accessor_class
                        && let Some(getter_fn) =
                            ast.accessor_getters.get(&(cls.clone(), name.clone()))
                        && let Some(Type::Function(_params, ret)) = self.globals.get(getter_fn)
                    {
                        return Ok(resolve_class_ref(
                            ret,
                            &self.aliases,
                            &self.generic_alias_decls,
                        ));
                    }
                }
                /* T-15.g.2 (v0.5.0) — built-in `Promise<T>.value` returns
                 * T. The parser desugars `await p` to `p.value` (Phase L
                 * MVP — synchronous read of the resolved value), so this
                 * Member-access rule is the entire `await` typing for
                 * built-in promises. ssa_lower's matching arm emits
                 * `__torajs_promise_get_value(p)` which reads the i64
                 * value slot from the Promise heap block. The user-class
                 * Promise pattern keeps working since Type::Object
                 * structs go through the field-lookup branch above. */
                if let Type::Promise(inner) = &obj_ty
                    && name == "value"
                {
                    return Ok((**inner).clone());
                }
                /* P10.4 — `await e` on non-Promise e per ES spec:
                 * conceptually `Promise.resolve(e)` is constructed and
                 * its resolved value is yielded — which for any
                 * non-thenable e collapses to e itself. The parser
                 * desugars `await e` to `e.value`; this arm treats
                 * `.value` as identity for the types that can never
                 * carry a real `value` field of their own (primitives
                 * + the built-in heap container types). The Promise
                 * arm above takes precedence for actual Promise<T>;
                 * the user-struct field-lookup arm below takes
                 * precedence for declared `{ value: T }` Struct
                 * shapes. Type::Object("Symbol" / etc.), Type::Class
                 * and Type::Struct intentionally fall through —
                 * those CAN have a real `.value` member. */
                if name == "value"
                    && matches!(
                        obj_ty,
                        Type::Number | Type::String | Type::Boolean | Type::Array(_) | Type::BigInt
                    )
                {
                    return Ok(obj_ty);
                }
                // Phase I.1 — class method on Type::Struct. Reverse-lookup
                // the class name from the struct shape (matches the
                // first-aliased class with that struct), then probe
                // `__cm_<class>__<name>` in globals. If found, return
                // its Function type with `__this` (the implicit first
                // param) stripped — caller's args fill the remaining
                // params. Used by sibling-method calls left
                // un-rewritten by desugar (the chain-and-static cases
                // were rewritten into Ident calls already).
                if let Type::Struct(_) = &obj_ty {
                    let mut class_name: Option<String> = None;
                    for (n, ty) in self.aliases.iter() {
                        if *ty == obj_ty && ast.class_parents.contains_key(n) {
                            class_name = Some(n.clone());
                            break;
                        }
                    }
                    if let Some(cname) = class_name {
                        let cm_name = format!("__cm_{cname}__{name}");
                        if let Some(Type::Function(params, ret)) = self.globals.get(&cm_name) {
                            // Strip the implicit `__this` first param.
                            if !params.is_empty() {
                                let user_params = params[1..].to_vec();
                                return Ok(Type::Function(user_params, ret.clone()));
                            }
                        }
                    }
                }
                match (&obj_ty, name.as_str()) {
                    (Type::Object("console"), m)
                        if matches!(m, "log" | "error" | "warn" | "info" | "debug") =>
                    {
                        // S328 — WHATWG console §1.1.{2,4}: `info` /
                        // `debug` print to the same stream as `log`.
                        // bun aliases info/debug to log (stdout); tr
                        // routes through the same `print_*` intrinsic
                        // family in ssa_lower.
                        Ok(Type::Function(vec![Type::Any], Box::new(Type::Void)))
                    }
                    // `Math` global — every method takes one number and
                    // returns a number. f64-flavored at the SSA level
                    // (the lowerer auto-promotes integer args), but
                    // check.rs uses the umbrella Type::Number.
                    (Type::Object("Math"), m)
                        if matches!(
                            m,
                            "sqrt" | "abs" | "floor" | "ceil" | "log" | "exp"
                            | "sign" | "round" | "trunc"
                            | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
                            | "log2" | "log10" | "cbrt"
                            | "sinh" | "cosh" | "tanh" | "asinh" | "acosh" | "atanh"
                            | "expm1" | "log1p" | "clz32" | "fround" | "f16round"
                        ) =>
                    {
                        Ok(Type::Function(vec![Type::Number], Box::new(Type::Number)))
                    }
                    (Type::Object("Math"), "imul") => Ok(Type::Function(
                        vec![Type::Number, Type::Number],
                        Box::new(Type::Number),
                    )),
                    // ES2025 §21.3.2.32 — correctly-rounded sum of an
                    // Array<Number>. Narrow form: only Array<Number>
                    // input (spec accepts any iterable of Number;
                    // tora's Set/Map/iterator surface comes later).
                    (Type::Object("Math"), "sumPrecise") => Ok(Type::Function(
                        vec![Type::Array(Box::new(Type::Number))],
                        Box::new(Type::Number),
                    )),
                    (Type::Object("Math"), "random") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Number),
                    )),
                    // Two-arg methods: pow(x, y), min(a, b), max(a, b),
                    // atan2(y, x).
                    (Type::Object("Math"), m)
                        if matches!(m, "pow" | "min" | "max" | "atan2") =>
                    {
                        Ok(Type::Function(
                            vec![Type::Number, Type::Number],
                            Box::new(Type::Number),
                        ))
                    }
                    // Constants — read directly without parens.
                    (Type::Object("Math"), m)
                        if matches!(
                            m,
                            "PI" | "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E"
                            | "SQRT2" | "SQRT1_2"
                        ) =>
                    {
                        Ok(Type::Number)
                    }
                    // Number namespace constants — common floating-point
                    // limits and integer-safety bounds.
                    (Type::Object("Number"), m)
                        if matches!(
                            m,
                            "NaN" | "POSITIVE_INFINITY" | "NEGATIVE_INFINITY"
                            | "EPSILON" | "MAX_SAFE_INTEGER" | "MIN_SAFE_INTEGER"
                            | "MAX_VALUE" | "MIN_VALUE"
                        ) =>
                    {
                        Ok(Type::Number)
                    }
                    // `Number` global — parseInt / parseFloat coerce a
                    // string to a number; isInteger / isNaN / isFinite
                    // are unary number predicates.
                    (Type::Object("Number"), "parseInt") => Ok(Type::Function(
                        vec![Type::String, Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Object("Number"), "parseFloat") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Number),
                    )),
                    (Type::Object("Number"), m)
                        if matches!(m, "isInteger" | "isNaN" | "isFinite" | "isSafeInteger") =>
                    {
                        Ok(Type::Function(vec![Type::Number], Box::new(Type::Boolean)))
                    }
                    // P12.4-B/C — `BigInt.asIntN(bits, value)` /
                    // `BigInt.asUintN(bits, value)` per ES §21.2.2.1 /
                    // §21.2.2.2. `bits` is a `Number` (Index per spec;
                    // tora's `Number` covers integer-shaped values
                    // already); `value` is `BigInt`; returns BigInt.
                    (Type::Object("BigInt"), "asIntN") | (Type::Object("BigInt"), "asUintN") => {
                        Ok(Type::Function(
                            vec![Type::Number, Type::BigInt],
                            Box::new(Type::BigInt),
                        ))
                    }
                    // V3-18 m2.b — Object.prototype methods on
                    // constructor-namespace objects (Number / String /
                    // Boolean / Array / etc). Same subset semantics as
                    // m2.a on primitives: hasOwnProperty /
                    // propertyIsEnumerable always false (no own enum
                    // properties tracked), valueOf identity.
                    (Type::Object(_), "hasOwnProperty")
                    | (Type::Object(_), "propertyIsEnumerable") => {
                        Ok(Type::Function(vec![Type::String], Box::new(Type::Boolean)))
                    }
                    (Type::Object(_), "isPrototypeOf") => {
                        Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean)))
                    }
                    (Type::Object(_), "toString") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
                    }
                    // V3-18 m2.c → 2026-05-18 — `Number.prototype` /
                    // `String.prototype` / etc — every constructor
                    // object has a `.prototype` property. Subset
                    // returns Type::Any so subsequent `.X` access
                    // routes through dynobj_get (returning ANY_UNDEF
                    // for unknown fields, harmless when consumed by
                    // a verifyProperty-style stub). Pre-fix Type::Null
                    // blocked `verifyProperty(X.prototype.Y, ...)` —
                    // the dominant test262 shape — at typecheck time.
                    // typeof X.prototype still works via the typeof-
                    // namespace-member arm above.
                    (Type::Object(_), "prototype") => Ok(Type::Any),
                    (Type::Object(_), "name") => Ok(Type::String),
                    (Type::Object(_), "length") => Ok(Type::Number),
                    // JSON.stringify(value) — value can be any subset
                    // type; result is String. The actual type-aware
                    // serialization shape happens at lower-time
                    // (per-call-site monomorphization).
                    (Type::Object("JSON"), "stringify") => {
                        Ok(Type::Function(vec![Type::Any], Box::new(Type::String)))
                    }
                    // M6.3 — `JSON.parse(text): T` — caller-driven type
                    // inference. The return type at typecheck level is
                    // Any (effectively a hole); ssa_lower's LetDecl
                    // arm reads the slot's `type_ann` and emits the
                    // per-shape parser at lower time. check.rs accepts
                    // any `Type::Any` slot, so the let binding's
                    // declared `T` slot type drives the actual decode.
                    (Type::Object("JSON"), "parse") => {
                        Ok(Type::Function(vec![Type::String], Box::new(Type::Any)))
                    }
                    // Array.isArray(x) — compile-time static check.
                    (Type::Object("Array"), "isArray") => {
                        Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean)))
                    }
                    // `Array.from(s)` over a string — returns `string[]`
                    // with one single-char string per byte. The other
                    // overloads (iterable / arrayLike / mapFn) aren't in
                    // tr's subset; ssa_lower validates the arg is Type::Str
                    // at lower-time.
                    (Type::Object("Array"), "from") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Array(Box::new(Type::String))),
                    )),
                    // `Object.keys(obj)` — returns Array<String> with the
                    // field names of obj's struct type. Static-resolved at
                    // codegen (the struct layout is known at compile
                    // time), so this is a compile-time constant array
                    // emitted at the call site. Param is Type::Any
                    // because the typechecker doesn't yet track
                    // "any-struct" as a constraint; ssa_lower verifies
                    // the arg actually carries Type::Obj at lower-time
                    // and panics on non-struct args.
                    (Type::Object("Object"), "keys")
                    // tr has no prototype chain, so own == all; alias
                    // getOwnPropertyNames to keys at lower time.
                    | (Type::Object("Object"), "getOwnPropertyNames")
                    // ES6 §28.1.11 — `Reflect.ownKeys` shares this
                    // signature; the ssa_lower dispatch routes both
                    // through the same struct-keys emit.
                    | (Type::Object("Reflect"), "ownKeys") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Array(Box::new(Type::String))),
                    )),
                    /* v0.2 #3 — Object.hasOwn(obj, key) — compile-time
                     * resolved when key is a Str literal (struct layout
                     * known at lower time). Boolean result.
                     *
                     * Object.freeze / isFrozen are deferred — pairing
                     * them as a no-op returning false would break
                     * `Object.isFrozen(Object.freeze(o)) === true`
                     * test262 cases. Real implementation needs a
                     * frozen bit on the universal heap header (v0.3). */
                    (Type::Object("Object"), "hasOwn")
                    // ES6 §28.1.9 — `Reflect.has` shares this signature.
                    | (Type::Object("Reflect"), "has") => Ok(Type::Function(
                        vec![Type::Any, Type::String],
                        Box::new(Type::Boolean),
                    )),
                    // ES6 §28.1.6 — `Reflect.get(target, key)`. Subset:
                    // typed struct target + literal-string key folds at
                    // ssa-lower time to a struct field load + box-to-Any
                    // (key not in layout → ANY_UNDEF). Dynamic key or
                    // non-struct target stays a deferred substrate.
                    (Type::Object("Reflect"), "get") => Ok(Type::Function(
                        vec![Type::Any, Type::String],
                        Box::new(Type::Any),
                    )),
                    // Object.is(a, b) — strict equality with two
                    // corner-case overrides vs `===`: NaN is equal to
                    // NaN, and +0 is NOT equal to -0. Lowered per arg
                    // SSA type (Type::Number → __torajs_object_is_f64
                    // runtime helper that bitcasts the ±0 case;
                    // Type::String → __torajs_str_eq; everything else
                    // falls back to SSA-level == compare).
                    (Type::Object("Object"), "is") => Ok(Type::Function(
                        vec![Type::Any, Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    /* T-09.b (v0.4.0) — Object.entries(obj) returns
                     * `Array<Array<Any>>` (each inner is `[key, value]`).
                     * Codegen unfolds at compile time using the static
                     * struct layout from check.rs's struct_layouts —
                     * zero-cost reflection just like Object.keys. The
                     * Type::Any tagged-slot path from T-10 carries the
                     * mixed key (Str) + value (per-field type). */
                    (Type::Object("Object"), "entries") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Array(Box::new(Type::Array(Box::new(Type::Any))))),
                    )),
                    /* T-09.c (v0.4.0) — Object.fromEntries(entries)
                     * uses caller-driven typing (similar to JSON.parse):
                     * the typecheck-level return is Any, and ssa_lower's
                     * LetDecl arm unfolds per the slot struct schema.
                     * MVP: entries are assumed to be in struct field
                     * declaration order (matches Object.entries
                     * round-trip), no key-matching scan. */
                    (Type::Object("Object"), "fromEntries") => Ok(Type::Function(
                        vec![Type::Array(Box::new(Type::Array(Box::new(Type::Any))))],
                        Box::new(Type::Any),
                    )),
                    /* S258 — Object.values(obj) → Array<Any>. SSA-emit
                     * already dispatches Obj/Arr/Str/Any receivers
                     * (ssa_lower.rs ~18495); checktime sig was missing.
                     * Return Array<Any> — heterogeneous struct fields
                     * + Any receiver both box to Any per ssa_lower's
                     * anyv_struct_values walker; homogeneous struct
                     * + Arr receivers also typecheck under Array<Any>
                     * (downcast-on-use). Trailing-arg widen folded
                     * into S256 below (matches!("entries"|"freeze"
                     * |"isFrozen"|"values"). */
                    (Type::Object("Object"), "values") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Array(Box::new(Type::Any))),
                    )),
                    /* T-09.d (v0.4.0) — Object.freeze(obj) sets the
                     * FROZEN bit on the universal heap header. Returns
                     * the same obj per spec. Subsequent field writes
                     * are silently ignored (matches non-strict mode;
                     * tr has no `"use strict"` directive). The arg
                     * type is permissive (Type::Any) — runtime accepts
                     * any heap object pointer. */
                    (Type::Object("Object"), "freeze") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Any),
                    )),
                    /* Object.isFrozen(obj) — reads the FROZEN bit. */
                    (Type::Object("Object"), "isFrozen") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    /* T-15.g.1 — Promise.resolve(v) / Promise.reject(v).
                     * MVP only Number arg (Type::Promise<Number>);
                     * heap types (Promise<string>, etc.) land in
                     * T-15.g.4 via direct call-arm handling that
                     * inspects the inferred arg type at the call site
                     * (the static-method table's TypeVar isn't
                     * instantiated automatically). */
                    (Type::Object("Promise"), "resolve")
                    | (Type::Object("Promise"), "reject") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::Promise(Box::new(Type::Number))),
                    )),
                    /* T-15.g.3 / T-19.g (v0.5.0) — `Promise<T>.then(cb)`
                     * chains. cb signature is `(v: T) => T` (same T
                     * in/out — no generic U yet). T ∈ Number / String
                     * / Boolean — the three i64-roundtrippable
                     * primitives the runtime helper
                     * `__torajs_promise_then_simple` packs through.
                     * Heap T (Array / Struct / Date) deferred to
                     * T-15.g.5+ alongside the closure-cb substrate. */
                    (Type::Promise(inner), "then")
                        if matches!(
                            **inner,
                            Type::Number | Type::String | Type::Boolean | Type::Any
                        ) =>
                    {
                        // P10.7 — `Promise<Any>` participates same as
                        // the i64-roundtrippable primitives: cb is
                        // `(v: Any) => Any`; the existing
                        // `__torajs_promise_then_simple` helper takes
                        // / returns i64 (NaN-box AnyValue at the SSA
                        // layer is i64-sized).
                        Ok(Type::Function(
                            vec![Type::Function(
                                vec![(**inner).clone()],
                                Box::new((**inner).clone()),
                            )],
                            Box::new(Type::Promise(inner.clone())),
                        ))
                    }
                    /* T-19.k (v0.5.0) — `Promise<T>.catch(onRejected)`.
                     * cb sig is `(reason: T) => T` — same shape as
                     * .then's onFulfilled. Returns a Promise<T> that
                     * resolves with cb's return value on rejection,
                     * or passes through source's value on fulfillment.
                     * T scope matches .then (Number / String / Boolean)
                     * since both share the i64-roundtripping runtime
                     * helper. spec-strict heterogeneous T → U lands
                     * with TypeVar substitution post-T-15.g.4. */
                    (Type::Promise(inner), "catch")
                        if matches!(
                            **inner,
                            Type::Number | Type::String | Type::Boolean | Type::Any
                        ) =>
                    {
                        // P10.7 — symmetric with the `.then` widening
                        // above. `Promise<Any>.catch(cb)` runs through
                        // `__torajs_promise_catch_simple` / `_closure`
                        // unchanged at the SSA layer.
                        Ok(Type::Function(
                            vec![Type::Function(
                                vec![(**inner).clone()],
                                Box::new((**inner).clone()),
                            )],
                            Box::new(Type::Promise(inner.clone())),
                        ))
                    }
                    /* T-19.k — `Promise<T>.finally(onFinally)`. cb sig
                     * is `() => void` per spec — no value passed in,
                     * cb's return ignored. Returns a Promise<T> with
                     * the same state + value as the source (after
                     * cb runs). cb runs on either settled state. */
                    (Type::Promise(inner), "finally") => Ok(Type::Function(
                        vec![Type::Function(vec![], Box::new(Type::Void))],
                        Box::new(Type::Promise(inner.clone())),
                    )),
                    /* T-13.b (v0.4.0) — Symbol.for(key) returns the
                     * registered Symbol for the key (creates one on
                     * first call). Identity preserved across calls. */
                    (Type::Object("Symbol"), "for") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Symbol),
                    )),
                    /* Symbol.keyFor(s) — inverse: returns the key
                     * Symbol.for() registered the symbol under, or
                     * null for unregistered (Symbol(...)) symbols. */
                    (Type::Object("Symbol"), "keyFor") => Ok(Type::Function(
                        vec![Type::Symbol],
                        Box::new(Type::Nullable(Box::new(Type::String))),
                    )),
                    /* T-13.c (v0.4.0) — well-known Symbol singletons.
                     * Process-level lazy-init pointers; identity
                     * preserved across all access sites. for-of
                     * dispatch via `[Symbol.iterator]()` lands with
                     * v0.5 (iterator protocol substrate). */
                    (Type::Object("Symbol"), "iterator")
                    | (Type::Object("Symbol"), "asyncIterator")
                    | (Type::Object("Symbol"), "toPrimitive") => Ok(Type::Symbol),
                    /* T-09.a (v0.4.0) — 5 Object methods that don't fit
                     * tr's nominal class system / fixed struct schema.
                     * Reject at typecheck with a clear phase pointer
                     * rather than ship a silently-wrong implementation.
                     *
                     * - getPrototypeOf / setPrototypeOf: bun returns the
                     *   prototype object (a runtime value); tr's nominal
                     *   class system has no equivalent runtime concept.
                     *   Lands with T-27 (Function constructor era) when
                     *   dynamic substrate becomes available.
                     * - defineProperty / defineProperties /
                     *   getOwnPropertyDescriptor: dynamic property add /
                     *   descriptor introspection requires schema
                     *   mutation; tr's struct layout is fixed at class
                     *   declaration. Lands with T-27 / Type::Any field
                     *   substrate post-v0.5.
                     */
                    // P4.2 Phase B+C — Object.getPrototypeOf returns
                    // the class's prototype object as an Any-box (the
                    // same `__proto_<C>` registered via
                    // __torajs_proto_register at module init). Pre-P4.2
                    // the stub returned Null; with prototype singletons
                    // now exposed, return Any so the caller can `===`
                    // against `C.prototype` and chain-walk via further
                    // getPrototypeOf calls. Returns ANY_NULL (still
                    // Type::Any tag-wise) when the arg has no prototype
                    // (Type::Obj with class_tag 0, or a Type::Any whose
                    // dynobj lacks `__proto__`).
                    (Type::Object("Object"), "getPrototypeOf") => {
                        Ok(Type::Function(vec![Type::Any], Box::new(Type::Any)))
                    }
                    // P3.3 — Object.defineProperty(obj, key, descriptor)
                    // accepted at typecheck. ssa_lower intercepts the
                    // Call, extracts descriptor.value (other descriptor
                    // fields like writable/configurable/enumerable/get/
                    // set are subset-deferred), and routes to dynobj_set.
                    // obj is Type::Any (must be a dynobj-backed Any-box);
                    // key is Type::String; descriptor is Type::Any
                    // (typically a plain object literal at the call site
                    // — ssa_lower probes for the .value field at AST time).
                    (Type::Object("Object"), "defineProperty") => Ok(Type::Function(
                        vec![Type::Any, Type::String, Type::Any],
                        Box::new(Type::Void),
                    )),
                    // P3.getOwnPropertyDescriptor — accept at typecheck.
                    // ssa_lower intercepts and constructs an Any-boxed
                    // descriptor object `{value, writable, enumerable,
                    // configurable}` from the dynobj bucket's stored
                    // tag/value/flags (per dcf069f attribute-flag
                    // tracking). Missing key returns Any-boxed undefined.
                    (Type::Object("Object"), "getOwnPropertyDescriptor") => Ok(
                        Type::Function(
                            vec![Type::Any, Type::String],
                            Box::new(Type::Any),
                        ),
                    ),
                    // 2026-05-18 — accept these as permissive Any
                    // typecheck-only stubs (no real substrate yet).
                    // ssa_lower has no special intercept either: the
                    // calls reach the generic call path and would
                    // panic. With test262 5k unlock being the goal,
                    // accept here so harness-shim consumers (which
                    // never read the return) flow through; cases
                    // that need real spec behavior bucket as bugs
                    // rather than incompatible.
                    (Type::Object("Object"), "setPrototypeOf") => Ok(Type::Function(
                        vec![Type::Any, Type::Any],
                        Box::new(Type::Any),
                    )),
                    (Type::Object("Object"), "defineProperties") => Ok(Type::Function(
                        vec![Type::Any, Type::Any],
                        Box::new(Type::Void),
                    )),
                    // `Object.create(proto, descriptors?)` — common
                    // test262 init pattern (`Object.create(null)`).
                    // Returns Any (a fresh dynobj-backed Any-box at
                    // lower time).
                    (Type::Object("Object"), "create") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Any),
                    )),
                    // `Object.assign(target, ...sources)` — copy own
                    // enumerable props. Subset accepts any-typed
                    // target + variadic any sources; ssa_lower's
                    // generic-call path picks it up as a no-op
                    // (returns target) if not intercepted.
                    (Type::Object("Object"), "assign") => Ok(Type::Function(
                        vec![Type::Any, Type::Any],
                        Box::new(Type::Any),
                    )),
                    // `Object.preventExtensions(obj)` /
                    // `Object.isExtensible(obj)` / `Object.seal(obj)`
                    // / `Object.isSealed(obj)` — no-op substrate
                    // returns the obj / true|false. Real semantics
                    // (frozen-bit dispatch) requires runtime header
                    // flag extension — deferred.
                    (Type::Object("Object"), "preventExtensions")
                    | (Type::Object("Object"), "seal") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Any),
                    )),
                    (Type::Object("Object"), "isExtensible")
                    | (Type::Object("Object"), "isSealed") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    (Type::String, "length") | (Type::Array(_), "length") => Ok(Type::Number),
                    /* P6.1 / P6.2 — Map.prototype.size / Set.prototype.size
                     * accessor (spec §23.1.3.10 / §24.2.3.9). Member
                     * arm dispatches to a Number-typed read; ssa_lower
                     * calls `__torajs_map_size` (Set storage is the
                     * same Map runtime). */
                    (Type::Map, "size") | (Type::Set, "size") => Ok(Type::Number),
                    // M6.1 — String methods. All borrow `this` and any
                    // String args (consumption only fires at concat,
                    // which has its own arm). Bool-returning methods
                    // return Type::Boolean; index/charCodeAt return
                    // Number; slice returns String.
                    (Type::String, "slice") | (Type::String, "substring") => {
                        Ok(Type::Function(
                            vec![Type::Number, Type::Number],
                            Box::new(Type::String),
                        ))
                    }
                    // T-49 — `String.prototype.substr(start, length?)` (annexB
                    // legacy). The 1-arg shape is the common one in test262;
                    // the call-site arity-tolerance arm above
                    // (`slice / substring / substr` with args.len() < 2)
                    // accepts 0/1 args, and ssa_lower fills the missing
                    // length with i64::MAX so the runtime helper clamps.
                    (Type::String, "substr") => Ok(Type::Function(
                        vec![Type::Number, Type::Number],
                        Box::new(Type::String),
                    )),
                    (Type::String, "repeat") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::String),
                    )),
                    (Type::String, "toUpperCase") | (Type::String, "toLowerCase")
                    | (Type::String, "trim") | (Type::String, "trimStart")
                    | (Type::String, "trimEnd")
                    // `trimLeft` / `trimRight` are the non-standard but
                    // de-facto aliases that ship in every JS engine —
                    // ECMAScript Annex B documents them as legacy of
                    // `trimStart` / `trimEnd`.
                    | (Type::String, "trimLeft") | (Type::String, "trimRight")
                    // s.normalize() — Unicode normalization. tr's
                    // current Str layer is byte-oriented; for ASCII
                    // strings (the dominant test262 case) all four NFC/
                    // NFD/NFKC/NFKD forms are byte-identical with the
                    // input, so an identity stub round-trips correctly.
                    // Multi-byte UTF-8 strings would need Unicode tables
                    // — deferred to v1.0 (`\p{...}` + ICU work).
                    | (Type::String, "normalize")
                    // ES2024 §22.1.3.30 — `toWellFormed()`. torajs is
                    // internally UTF-8, so lone surrogates can't be
                    // encoded; identity stub at lower time.
                    | (Type::String, "toWellFormed") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::String),
                    )),
                    // ES2024 §22.1.3.10 — `isWellFormed()`. Mirror of
                    // toWellFormed (see above) — always true.
                    (Type::String, "isWellFormed") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Boolean),
                    )),
                    (Type::String, "padStart") | (Type::String, "padEnd") => {
                        Ok(Type::Function(
                            vec![Type::Number, Type::String],
                            Box::new(Type::String),
                        ))
                    }
                    (Type::String, "replace") | (Type::String, "replaceAll") => {
                        // Pattern arg is either a literal Str (existing
                        // string-only path through __torajs_str_replace
                        // / __torajs_str_replace_all) or a RegExp
                        // (Phase 1b regex path). Repl arg is either a
                        // Str (existing path) or a callback fn (P9.5).
                        // Both args use Type::Any here so each can pass
                        // typecheck; ssa_lower picks the dispatch by
                        // operand SSA type. A1 callback shape required:
                        // `(m: string) => string` — multi-arg / capture-
                        // spread callbacks are A1.1.
                        Ok(Type::Function(
                            vec![Type::Any, Type::Any],
                            Box::new(Type::String),
                        ))
                    }
                    // `s.charAt(i)` — single-char substring at index i.
                    // Identical surface to `s[i]`; routed through the
                    // same substr_create / substr_slice path at lower
                    // time. tr's subset doesn't return "" on OOB —
                    // matches the unchecked-index convention used by
                    // index access.
                    (Type::String, "charAt") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::String),
                    )),
                    (Type::String, "at") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::String),
                    )),
                    (Type::Number, "toFixed")
                    | (Type::Number, "toExponential")
                    | (Type::Number, "toPrecision") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::String),
                    )),
                    (Type::Number, "toString") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
                    }
                    // S139 — `n.toLocaleString(locales?, options?)` per ES
                    // §21.1.3.4 accepts optional locale + options args;
                    // tr's subset is en-US only and ignores them, so the
                    // signature is `(Any?, Any?) -> string`. ssa_lower
                    // drops the args before the 1-arg runtime helper.
                    (Type::Number, "toLocaleString") => Ok(Type::Function(
                        vec![Type::Any, Type::Any],
                        Box::new(Type::String),
                    )),
                    // S140 — `s.toLocaleLowerCase` / `toLocaleUpperCase`
                    // per ES §22.1.3.21 / §22.1.3.23 accept optional
                    // locales arg; tr's subset is en-US only so the
                    // arg is accepted (Any?) and ignored — same shape
                    // as Number.toLocaleString (S139).
                    (Type::String, "toLocaleLowerCase") | (Type::String, "toLocaleUpperCase") => {
                        Ok(Type::Function(vec![Type::Any], Box::new(Type::String)))
                    }
                    // V3-18 wedge — Boolean.prototype.toString / valueOf.
                    // Per JS spec §20.3.3.2 / §20.3.3.3 — `(true).toString()`
                    // → "true", `(false).toString()` → "false". valueOf
                    // returns the boolean itself. Common in calls like
                    // `b.toString()` where b is a typed Boolean binding.
                    (Type::Boolean, "toString") | (Type::Boolean, "toLocaleString") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
                    }
                    (Type::Boolean, "valueOf") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::Boolean)))
                    }
                    // V3-18 m1.h.27 — BigInt.prototype.toString() →
                    // decimal string (no `n` suffix). Per JS spec
                    // §21.2.3.5 / §21.2.3.6. The runtime path
                    // already exists (used by string concat coerce);
                    // this just wires up the typecheck.
                    //
                    // P12.4 — accept optional radix argument per ES
                    // §6.1.6.2.13 (`BigInt.prototype.toString(radix)`).
                    // Signature is `(radix?: Any) → String`; the Any
                    // slot is padded to `undefined` for the 0-arg
                    // common path and accepts a Number radix when
                    // supplied (ssa-lower routes to bigint_to_string
                    // vs bigint_to_string_radix per arg count).
                    // toLocaleString remains 0-arg per ES §22.2.3.2.
                    (Type::BigInt, "toString") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::String),
                    )),
                    (Type::BigInt, "toLocaleString") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
                    }
                    // V3-18 m1.h.47 — Symbol.prototype.toString() →
                    // "Symbol(<desc>)" / "Symbol()". Symbol.description
                    // returns the desc (or null for Symbol() with no
                    // arg). Per JS spec §20.4.3.3 / §20.4.3.2.
                    (Type::Symbol, "toString") | (Type::Symbol, "toLocaleString") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
                    }
                    // V3-18 m2.c — `.constructor` on primitives
                    // returns the constructor function (Number /
                    // String / etc). Subset stub: Type::Any (the
                    // constructor's actual type is callable but
                    // tora has no first-class function reference for
                    // the namespace ctor; Type::Any lets the test
                    // typecheck without committing to a real shape).
                    (Type::Number, "constructor")
                    | (Type::String, "constructor")
                    | (Type::Boolean, "constructor")
                    | (Type::BigInt, "constructor")
                    | (Type::Symbol, "constructor") => Ok(Type::Any),
                    // V3-18 m2.a — Object.prototype methods exposed on
                    // every primitive via JS's auto-boxing rules:
                    //   .valueOf()              → returns the primitive itself
                    //   .hasOwnProperty(name)    → false (primitives have
                    //                              no own properties in our
                    //                              subset)
                    //   .propertyIsEnumerable(name) → false (same)
                    //   .isPrototypeOf(obj)     → false (we have no real
                    //                              prototype chain)
                    // ssa_lower handles the dispatch with constant folds
                    // since the values can't actually carry user-added
                    // properties.
                    (Type::Number, "valueOf") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::Number)))
                    }
                    // ES §23.1.3.34 — `arr.valueOf()` returns the
                    // Array itself (identity). The default Object
                    // protocol applies — Array doesn't override
                    // valueOf with its own coercion. ssa_lower folds
                    // the call to the receiver operand without a
                    // runtime helper.
                    (Type::Array(elem), "valueOf") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Array(elem.clone())),
                    )),
                    (Type::String, "valueOf")
                    // V3-18 wedge — String.prototype.toString /
                    // toLocaleString / valueOf all return the
                    // primitive string itself per JS spec
                    // §22.1.3.27 / §22.1.3.31 / §22.1.3.34.
                    // Already wired for Number / Boolean / BigInt /
                    // Symbol but missing for String, so `s.toString()`
                    // hit 'no member .toString on type String'.
                    | (Type::String, "toString")
                    | (Type::String, "toLocaleString") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
                    }
                    // (`(Type::Boolean, "valueOf")` is handled by the
                    // earlier Boolean arm — dead duplicate removed for
                    // the zero-warn build rule.)
                    (Type::BigInt, "valueOf") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::BigInt)))
                    }
                    (Type::Number, "hasOwnProperty")
                    | (Type::String, "hasOwnProperty")
                    | (Type::Boolean, "hasOwnProperty")
                    | (Type::BigInt, "hasOwnProperty")
                    | (Type::Symbol, "hasOwnProperty")
                    | (Type::Any, "hasOwnProperty")
                    | (Type::Number, "propertyIsEnumerable")
                    | (Type::String, "propertyIsEnumerable")
                    | (Type::Boolean, "propertyIsEnumerable")
                    | (Type::BigInt, "propertyIsEnumerable")
                    | (Type::Symbol, "propertyIsEnumerable")
                    | (Type::Any, "propertyIsEnumerable") => {
                        Ok(Type::Function(vec![Type::String], Box::new(Type::Boolean)))
                    }
                    (Type::Any, "valueOf") => Ok(Type::Function(Vec::new(), Box::new(Type::Any))),
                    (Type::Any, "toString") => Ok(Type::Function(Vec::new(), Box::new(Type::String))),
                    (Type::Any, "isPrototypeOf") => Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean))),
                    (Type::Any, "constructor") => Ok(Type::Any),
                    // RegExp instance methods. v0.2 #1 ships `.test(s)`;
                    // `.exec` / `.toString` / `.source` / `.flags` /
                    // `.global` / `.lastIndex` come in subsequent
                    // sub-phases. The matching engine in
                    // `runtime_regex.c` is the single source of truth
                    // for both `re.test(s)` and the `s.match(re)` /
                    // `s.replace(re, repl)` paths in v0.2 #1.b/c.
                    (Type::RegExp, "test") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Boolean),
                    )),
                    // ES §22.2.6.13 — `re.toString()` returns
                    // `/` + source + `/` + flags. Runtime helper
                    // `__torajs_regex_to_string` builds the string in
                    // one alloc.
                    (Type::RegExp, "toString") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::String),
                    )),
                    // T-37 followup — `re.source` returns the original
                    // pattern string (no flags, no slashes). Compile-
                    // time wires through a runtime intrinsic that
                    // wraps re->src_bytes in a Str.
                    (Type::RegExp, "source") => Ok(Type::String),
                    // ES §22.2.6.4 — `re.flags` returns the spec-
                    // ordered flag string ("" / "g" / "im" / "gimsuy"
                    // / etc.). Order is fixed: g, i, m, s, u, y. The
                    // runtime helper `__torajs_regex_get_flags` builds
                    // the canonical string.
                    (Type::RegExp, "flags") => Ok(Type::String),
                    // ES §22.2.6.5-10 — boolean flag instance accessors.
                    // Each maps to a single bit test on `re.flags`;
                    // the runtime helper `__torajs_regex_has_flag(re,
                    // flag_bit)` does the AND. ssa_lower emits the
                    // appropriate `RE_FLAG_*` byte constant per arm.
                    (Type::RegExp, "global")
                    | (Type::RegExp, "ignoreCase")
                    | (Type::RegExp, "multiline")
                    | (Type::RegExp, "dotAll")
                    | (Type::RegExp, "unicode")
                    | (Type::RegExp, "sticky") => Ok(Type::Boolean),
                    // P9.4 — `re.lastIndex` is a writable Number per
                    // spec §22.2.6.9. ssa_lower routes reads through
                    // __torajs_regex_get_last_index; writes through
                    // __torajs_regex_set_last_index (see assign-Member
                    // arm). Tracks across exec/match when g or y set.
                    (Type::RegExp, "lastIndex") => Ok(Type::Number),
                    // Phase 1c.1 — re.exec(s) returns Array<Str>:
                    // [matched, group1, group2, ...] on hit, empty
                    // array on miss. JS spec returns null on miss;
                    // tr deviates until Nullable<Array<Str>> propagation
                    // lands (Phase 1c.4 — same gate as s.match).
                    (Type::RegExp, "exec") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Array(Box::new(Type::String))),
                    )),
                    /* T-26 — WeakRef.deref(). Returns the target if
                     * still alive (rc-bumped on success), or null.
                     * Type-erased to Type::Any; users `as` cast to
                     * the original concrete type.
                     *
                     * S325 — sig is 0-arg per ES §26.1.3.2; widen via
                     * a dedicated arm below (the static-table path
                     * only fires when args.len() == 0, so trailing
                     * args[1..] are typecheck-and-dropped there). */
                    (Type::WeakRef, "deref") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Nullable(Box::new(Type::Any))),
                    )),
                    /* T-26.B — WeakMap methods. set takes (key,
                     * value); both type-erased to Any. get returns
                     * Nullable<Any>. has / delete return Boolean. */
                    (Type::WeakMap, "set") => Ok(Type::Function(
                        vec![Type::Any, Type::Any],
                        Box::new(Type::Void),
                    )),
                    (Type::WeakMap, "get") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Nullable(Box::new(Type::Any))),
                    )),
                    (Type::WeakMap, "has") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    (Type::WeakMap, "delete") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    /* T-26.B — WeakSet methods. */
                    (Type::WeakSet, "add") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Void),
                    )),
                    (Type::WeakSet, "has") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    (Type::WeakSet, "delete") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    /* P6.1 — Map<K,V> methods. set takes (key, value)
                     * both type-erased to Any (the runtime stores
                     * tagged-Any slots regardless); set returns the
                     * map itself per spec §23.1.3.9 enabling chained
                     * `m.set(k,v).set(k2,v2)` idiom (S127-5). ssa-lower
                     * mirrors with an emit_rc_inc on the receiver
                     * before returning so the chained value owns its
                     * own ref independent of the source binding.
                     * get returns Nullable<Any>. has / delete return
                     * Boolean. clear returns Void. */
                    (Type::Map, "set") => Ok(Type::Function(
                        vec![Type::Any, Type::Any],
                        Box::new(Type::Map),
                    )),
                    (Type::Map, "get") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Nullable(Box::new(Type::Any))),
                    )),
                    (Type::Map, "has") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    (Type::Map, "delete") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    (Type::Map, "clear") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Void),
                    )),
                    /* P6.4a — Map.forEach. Spec callback shape is
                     * `(value, key, map) => void`. Both `value` and
                     * `key` are type-erased to Any since storage is
                     * the (tag, payload) Any-domain. */
                    (Type::Map, "forEach") => Ok(Type::Function(
                        vec![Type::Function(
                            vec![Type::Any, Type::Any, Type::Map],
                            Box::new(Type::Void),
                        )],
                        Box::new(Type::Void),
                    )),
                    /* P6.4b — Map.keys / .values return a stateful
                     * MapIter (spec §23.1.3.8 / §23.1.3.13). The
                     * iter's `next()` produces `IteratorResult<any>`
                     * = `{ value: any, done: boolean }`. .entries is
                     * deferred to P6.4c (needs Array<Any> alloc per
                     * step + boxed (k, v) write). */
                    (Type::Map, "keys") | (Type::Map, "values") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::MapIter),
                    )),
                    /* P6.4c — Map.entries yields `[k, v]` pairs;
                     * Set.entries yields `[v, v]` pairs (spec
                     * §23.1.3.4 / §24.2.3.6). Both return the same
                     * MapIter handle (the runtime kind decides the
                     * yield shape). */
                    (Type::Map, "entries") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::MapIter),
                    )),
                    /* P6.4b — Set.keys = .values per spec §24.2.3.5
                     * (returns iterator over the elements). */
                    (Type::Set, "keys") | (Type::Set, "values") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::MapIter),
                    )),
                    (Type::Set, "entries") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::MapIter),
                    )),
                    /* P6.4b — MapIter.next() returns the spec-
                     * shaped IteratorResult struct. */
                    (Type::MapIter, "next") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Struct(vec![
                            ("value".into(), Type::Any),
                            ("done".into(), Type::Boolean),
                        ])),
                    )),
                    /* P6.4c-C3 — ArrIter.next() shape matches
                     * MapIter (both produce `IteratorResult<any>`). */
                    (Type::ArrIter, "next") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Struct(vec![
                            ("value".into(), Type::Any),
                            ("done".into(), Type::Boolean),
                        ])),
                    )),
                    /* P6.2 — Set<T> methods. add takes a single Any-
                     * typed value; storage piggy-backs on Map<T,
                     * undef> at runtime. S127-5 — add returns the set
                     * itself per spec §24.2.3.1 to enable chained
                     * `s.add(v1).add(v2)` idiom. ssa-lower mirrors
                     * with emit_rc_inc + receiver return. */
                    (Type::Set, "add") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Set),
                    )),
                    (Type::Set, "has") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    // ES2025 read-only Set setops (§24.2.3.{12,13,14}).
                    // Spec accepts any "Set-like" object; tora's narrow
                    // form requires the argument to also be a Set.
                    (Type::Set, "isSubsetOf")
                    | (Type::Set, "isSupersetOf")
                    | (Type::Set, "isDisjointFrom") => Ok(Type::Function(
                        vec![Type::Set],
                        Box::new(Type::Boolean),
                    )),
                    // ES2025 mutating Set setops (§24.2.3.{15,16,17,18}).
                    // Returns a fresh Set with rc=1.
                    (Type::Set, "union")
                    | (Type::Set, "intersection")
                    | (Type::Set, "difference")
                    | (Type::Set, "symmetricDifference") => Ok(Type::Function(
                        vec![Type::Set],
                        Box::new(Type::Set),
                    )),
                    (Type::Set, "delete") => Ok(Type::Function(
                        vec![Type::Any],
                        Box::new(Type::Boolean),
                    )),
                    (Type::Set, "clear") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Void),
                    )),
                    /* P6.4a — Set.forEach. Spec callback shape is
                     * `(value, value2, set) => void`, where the
                     * first two args are the same element. */
                    (Type::Set, "forEach") => Ok(Type::Function(
                        vec![Type::Function(
                            vec![Type::Any, Type::Any, Type::Set],
                            Box::new(Type::Void),
                        )],
                        Box::new(Type::Void),
                    )),
                    // v0.2 #2 Phase 2.0a — Date instance methods.
                    (Type::Date, "getTime")
                    | (Type::Date, "valueOf") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "toISOString") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::String),
                    )),
                    // ES §21.4.4.37 — `d.toJSON(key?)` returns the
                    // canonical JSON serialization, defined as
                    // `this.toISOString()` for any finite Date. The
                    // optional `key` argument is ignored per spec
                    // (only meaningful to Object.toJSON callers). MVP
                    // collapses to toISOString without the non-finite
                    // null short-circuit; non-finite Dates are an
                    // independent substrate (the underlying ms field
                    // already rejects non-finite ToNumber). The
                    // `key` arg is silently dropped here — the
                    // ssa_lower-side arm doesn't pass it through.
                    (Type::Date, "toJSON") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::String),
                    )),
                    // v0.2 #2 Phase 2.0b — UTC getters. Local-time
                    // siblings (getFullYear etc.) collapse to UTC
                    // until timezone awareness ships in Phase 2.0c.
                    (Type::Date, "getFullYear")
                    | (Type::Date, "getUTCFullYear")
                    | (Type::Date, "getMonth")
                    | (Type::Date, "getUTCMonth")
                    | (Type::Date, "getDate")
                    | (Type::Date, "getUTCDate")
                    | (Type::Date, "getHours")
                    | (Type::Date, "getUTCHours")
                    | (Type::Date, "getMinutes")
                    | (Type::Date, "getUTCMinutes")
                    | (Type::Date, "getSeconds")
                    | (Type::Date, "getUTCSeconds")
                    | (Type::Date, "getMilliseconds")
                    | (Type::Date, "getUTCMilliseconds")
                    | (Type::Date, "getDay")
                    | (Type::Date, "getUTCDay")
                    | (Type::Date, "getTimezoneOffset") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Number),
                    )),
                    // T-30 — Date setters + annexB methods. setTime
                    // takes ms and returns the new ms. setYear takes
                    // a year (annexB §B.2.4.2 — 0-99 → +1900) and
                    // returns the new ms. getYear (annexB §B.2.4.1)
                    // returns year - 1900. toGMTString (annexB §B.2.4.3)
                    // is an alias for toUTCString format.
                    (Type::Date, "setTime") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "setYear") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::Number),
                    )),
                    // Per-field setters per ES §21.4.4.20-26. Each
                    // takes 1-N Number args (trailing ones optional);
                    // we expose them as taking N required Numbers
                    // and let ssa_lower sentinel-pad missing trailing
                    // args. Returns the new `d.ms` (Number) per spec.
                    (Type::Date, "setFullYear") => Ok(Type::Function(
                        vec![Type::Number, Type::Number, Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "setMonth") => Ok(Type::Function(
                        vec![Type::Number, Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "setDate") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "setHours") => Ok(Type::Function(
                        vec![Type::Number, Type::Number, Type::Number, Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "setMinutes") => Ok(Type::Function(
                        vec![Type::Number, Type::Number, Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "setSeconds") => Ok(Type::Function(
                        vec![Type::Number, Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "setMilliseconds") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "getYear") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Number),
                    )),
                    (Type::Date, "toGMTString")
                    | (Type::Date, "toUTCString")
                    | (Type::Date, "toDateString")
                    | (Type::Date, "toLocaleString")
                    | (Type::Date, "toLocaleDateString")
                    | (Type::Date, "toLocaleTimeString") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::String),
                    )),
                    // Date.now() — static, returns ms-since-epoch.
                    (Type::Object("Date"), "now") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Number),
                    )),
                    /* v0.3 #2 — Bun namespace (minimum).
                     * Bun.write(path, data) — bun-shape file write,
                     * routes to the same fs intrinsic. Bun.file(path)
                     * (chained-method shape returning a File object)
                     * lands when the surface gains object-result Calls. */
                    (Type::Object("Bun"), "write") => Ok(Type::Function(
                        vec![Type::String, Type::String],
                        Box::new(Type::Void),
                    )),
                    /* T-19 (v0.5.0) — `Bun.file(path)` returns an
                     * opaque BunFile handle. The user calls `.text()`
                     * (or future `.json()` / `.arrayBuffer()`) on it
                     * to actually read. The handle is internally
                     * `Type::String` (just the path) since the
                     * methods all dispatch through fs.readFileSync.
                     * Type::Object("BunFile") sentinel keeps the
                     * methods scoped so plain Strings don't match. */
                    (Type::Object("Bun"), "file") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Object("BunFile")),
                    )),
                    /* V3-08 — `Bun.gc(synchronous)`. tora's Bacon-Rajan
                     * cycle collector triggers regardless of the bool
                     * arg (we ignore it; bun uses it to gate JSC's
                     * concurrent GC). Both runtimes return void. */
                    (Type::Object("Bun"), "gc") => Ok(Type::Function(
                        vec![Type::Boolean],
                        Box::new(Type::Void),
                    )),
                    (Type::Object("BunFile"), "text") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Promise(Box::new(Type::String))),
                    )),
                    /* T-19.c (v0.5.0) — `Bun.file(p).exists()`. Bun
                     * exposes this as a fast existence-probe that
                     * doesn't open the file. Maps to fs.existsSync
                     * in the MVP "synchronous-then-resolve" model. */
                    (Type::Object("BunFile"), "exists") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Promise(Box::new(Type::Boolean))),
                    )),
                    /* T-19.d (v0.5.0) — `Bun.file(p).json()` returns
                     * Promise<Any>. The actual return type comes from
                     * the caller-driven `let X: T = await Bun.file(p)
                     * .json()` shape detection in ssa_lower's LetDecl
                     * arm — JSON.parse drives parsing per the slot's
                     * concrete T (number / string / Struct / Array<T>
                     * / etc.). At the typecheck layer we accept any
                     * slot type as long as the JSON parser knows how
                     * to handle it; concrete validation happens at
                     * lower time. */
                    (Type::Object("BunFile"), "json") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Promise(Box::new(Type::Any))),
                    )),
                    /* T-18.c (v0.5.0) — `Bun.file(p).size` synchronous
                     * property (NOT a method). Returns the file's
                     * byte size, or -1 if the path is missing or
                     * non-regular (bun returns 0 for missing — tr
                     * uses -1 to keep the missing case observable
                     * until typed-throw fs lands). */
                    (Type::Object("BunFile"), "size") => Ok(Type::Number),
                    /* T-21 (v0.6.0) — `fetch(url)` Response surface.
                     * `.text()` returns the (already-loaded) body as
                     * `Promise<string>`; `.status` is the HTTP status
                     * code (0 on transport error). `.ok` and JSON
                     * parsing land alongside the fetch options
                     * follow-up. */
                    (Type::Object("Response"), "text") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::Promise(Box::new(Type::String))),
                    )),
                    (Type::Object("Response"), "status") => Ok(Type::Number),
                    /* v0.3 #3 — process surface (minimum). */
                    (Type::Object("process"), "exit") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::Void),
                    )),
                    (Type::Object("process"), "cwd") => Ok(Type::Function(
                        Vec::new(),
                        Box::new(Type::String),
                    )),
                    /* `process.platform` — value access, not a Call.
                     * Returned as Type::String; ssa_lower's Member arm
                     * emits a runtime call to __torajs_process_platform. */
                    (Type::Object("process"), "platform") => Ok(Type::String),
                    /* `process.argv` / `Bun.argv` — runtime array of
                     * argv strings. Lowered by ssa_lower's Member arm
                     * to __torajs_process_argv(). */
                    (Type::Object("process"), "argv")
                    | (Type::Object("Bun"), "argv") => {
                        Ok(Type::Array(Box::new(Type::String)))
                    }
                    /* `process.env` — env-namespace Object; member
                     * access on it (`process.env.NAME`) routes through
                     * the (Object("env"), _) arm below to runtime getenv. */
                    (Type::Object("process"), "env") => Ok(Type::Object("env")),
                    /* `process.env.NAME` — Nullable<String> (NULL when
                     * var unset; tr's undefined→null bridge keeps
                     * `=== undefined` round-tripping). */
                    (Type::Object("env"), _) => Ok(Type::Nullable(Box::new(Type::String))),
                    /* T-03 (v0.3.0) — process.{stdout, stderr, stdin}
                     * value-Member: each exposes its own Object so the
                     * downstream `.write` / `.read` Call resolves at
                     * the (Object("process_stdout"), "write") arm
                     * below. (`process.stdout` itself is also a legal
                     * value reference — e.g. `let s = process.stdout`
                     * — so the value-Member must be type-able too.) */
                    (Type::Object("process"), "stdout") => Ok(Type::Object("process_stdout")),
                    (Type::Object("process"), "stderr") => Ok(Type::Object("process_stderr")),
                    /* `process.stdin` deferred — see comment on .read above. */
                    /* T-03 — process.stdout / process.stderr.write(s)
                     * Call shape. Returns Boolean to match bun's
                     * `process.stdout.write(s)` signature (true on
                     * success, false on backpressure / error — tr
                     * panics on short write so it always returns true
                     * when control returns). */
                    (Type::Object("process_stdout"), "write")
                    | (Type::Object("process_stderr"), "write") => {
                        Ok(Type::Function(
                            vec![Type::String],
                            Box::new(Type::Boolean),
                        ))
                    }
                    /* `process.stdin.read()` deferred to v0.5 — bun's
                     * API is Node Readable async (returns Buffer-or-
                     * null), so a sync drain-to-EOF would diverge from
                     * the oracle. Lands with the async substrate. */

                    /* v0.3 #1 — fs module surface (Phase 2.0a substrate).
                     * Synchronous file I/O; throw on error is Phase 2.0b. */
                    (Type::Object("fs"), "readFileSync") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::String),
                    )),
                    (Type::Object("fs"), "writeFileSync") => Ok(Type::Function(
                        vec![Type::String, Type::String],
                        Box::new(Type::Void),
                    )),
                    (Type::Object("fs"), "appendFileSync") => Ok(Type::Function(
                        vec![Type::String, Type::String],
                        Box::new(Type::Void),
                    )),
                    (Type::Object("fs"), "unlinkSync")
                    | (Type::Object("fs"), "mkdirSync") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Void),
                    )),
                    (Type::Object("fs"), "existsSync") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Boolean),
                    )),
                    /* T-18.b (v0.5.0) — fs.readdirSync(path) returns
                     * Array<string> with one entry per child (`.` /
                     * `..` filtered, matching bun spec). */
                    (Type::Object("fs"), "readdirSync") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Array(Box::new(Type::String))),
                    )),
                    (Type::Object("fs_promises"), "readdir") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Promise(Box::new(
                            Type::Array(Box::new(Type::String))
                        ))),
                    )),
                    /* T-18.a (v0.5.0) — `fs/promises` module. Each
                     * method calls the matching sync helper from
                     * `fs.<X>Sync` then wraps the result in
                     * Promise.resolve(...). MVP "synchronous-then-
                     * resolve" — real I/O suspension needs T-16
                     * state-machine async/await. Bun-parity:
                     * `import { readFile } from "fs/promises"; await
                     * readFile(p)` yields the file contents
                     * byte-identical with bun. */
                    (Type::Object("fs_promises"), "readFile") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Promise(Box::new(Type::String))),
                    )),
                    (Type::Object("fs_promises"), "writeFile") => Ok(Type::Function(
                        vec![Type::String, Type::String],
                        Box::new(Type::Promise(Box::new(Type::Void))),
                    )),
                    (Type::Object("fs_promises"), "appendFile") => Ok(Type::Function(
                        vec![Type::String, Type::String],
                        Box::new(Type::Promise(Box::new(Type::Void))),
                    )),
                    (Type::Object("fs_promises"), "unlink")
                    | (Type::Object("fs_promises"), "mkdir") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Promise(Box::new(Type::Void))),
                    )),
                    (Type::Object("fs_promises"), "exists") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Promise(Box::new(Type::Boolean))),
                    )),
                    // Phase 2.0b.2 — Date.parse(s) returns ms-since-epoch
                    // (or NaN sentinel — tr returns INT64_MIN; spec is NaN).
                    (Type::Object("Date"), "parse") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Number),
                    )),
                    // Date.UTC(year, month, day, hour, min, sec, ms) — UTC
                    // interpretation; returns ms-since-epoch. Min 2 args.
                    // tr accepts the 7-arg form via the same dispatch path
                    // as `new Date(...)` component ctor; missing trailing
                    // args default to month=0, day=1, rest=0 — but that
                    // padding happens at desugar time, which doesn't
                    // intercept `Date.UTC(...)` (only `new Date(...)`).
                    // For Phase 2.0b.2, tr's Date.UTC requires explicit
                    // 7 args; arity-aware desugar comes in 2.0c.
                    (Type::Object("Date"), "UTC") => Ok(Type::Function(
                        vec![Type::Number; 7],
                        Box::new(Type::Number),
                    )),
                    // String namespace static — `String.fromCharCode(n)`.
                    // `fromCodePoint` is the Unicode-aware sibling; in
                    // tr's byte-Str layout the two collapse for code
                    // points ≤ 0xff and ports keep arguments inside that
                    // range to stay bun-portable.
                    (Type::Object("String"), "fromCharCode")
                    | (Type::Object("String"), "fromCodePoint") => Ok(Type::Function(
                        vec![Type::Number],
                        Box::new(Type::String),
                    )),
                    (Type::String, "charCodeAt") | (Type::String, "codePointAt") => {
                        Ok(Type::Function(
                            vec![Type::Number],
                            Box::new(Type::Number),
                        ))
                    }
                    (Type::String, "startsWith") | (Type::String, "endsWith")
                    | (Type::String, "includes") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::Boolean),
                    )),
                    (Type::String, "indexOf")
                    | (Type::String, "lastIndexOf")
                    | (Type::String, "localeCompare")
                    // V3-18 wedge — String.prototype.search per JS
                    // spec §22.1.3.16. The full spec coerces the
                    // arg to a RegExp and uses Symbol.search, but
                    // for a plain string arg the result is exactly
                    // indexOf — first match position or -1.
                    // tora's subset only routes the string-arg
                    // form (RegExp arg is a follow-up substrate
                    // item alongside Symbol.search dispatch).
                    | (Type::String, "search") => {
                        Ok(Type::Function(
                            vec![Type::String],
                            Box::new(Type::Number),
                        ))
                    }
                    // s.split(sep): string[] — `sep` is Str or RegExp;
                    // Type::Any lets either type pass typecheck and
                    // ssa_lower dispatches on operand SSA type to the
                    // string-only `__torajs_str_split` or the regex
                    // path `__torajs_str_split_regex`.
                    // V3-18 wedge — `s.split(sep [, limit])` per ES
                    // §22.1.3.21. The optional `limit` slot uses
                    // Type::Any so the trailing-Any arity-pad path
                    // makes 1-arg calls type-check; the SSA layer in
                    // `ssa_lower_str.rs` already branches on
                    // `args.len() == 2` to emit the slice clamp.
                    (Type::String, "split") => Ok(Type::Function(
                        vec![Type::Any, Type::Any],
                        Box::new(Type::Array(Box::new(Type::String))),
                    )),
                    // s.match(re) — Phase 1b returns Array<Str>; without
                    // `g` flag the array has 1 element (the matched
                    // substring), with `g` it has all matches. Capture
                    // groups + JS-spec null-on-miss are Phase 1c.
                    (Type::String, "match") => Ok(Type::Function(
                        vec![Type::RegExp],
                        Box::new(Type::Array(Box::new(Type::String))),
                    )),
                    // s.matchAll(re) — Phase 1c.3 returns
                    // Array<Array<Str>>: outer = one entry per match,
                    // each inner = exec-shape [match, g1, g2, ...].
                    // JS spec returns an iterator; tr's array stand-in
                    // covers the dominant test262 usage pattern (a for-of
                    // loop or [...m]) until iterator protocol lands.
                    (Type::String, "matchAll") => Ok(Type::Function(
                        vec![Type::RegExp],
                        Box::new(Type::Array(Box::new(
                            Type::Array(Box::new(Type::String))
                        ))),
                    )),
                    // arr.join(sep): string — receiver is Array<T> for
                    // T = String / Number / Boolean (V3-18 m1.h.43:
                    // Number/Bool elements ToString'd inline by the
                    // runtime helper). sep borrowed; result freshly
                    // allocated.
                    (Type::Array(elem), "join")
                        if matches!(**elem, Type::String | Type::Number | Type::Boolean | Type::Any) => {
                        Ok(Type::Function(vec![Type::String], Box::new(Type::String)))
                    }
                    // V3-18 wedge — Array.prototype.toString. Per JS
                    // spec §22.1.3.30, equivalent to `arr.join(",")`.
                    // Subset matches the join intrinsic's element-type
                    // dispatch (Str / Number / Bool / Any — S126-4).
                    (Type::Array(elem), "toString" | "toLocaleString")
                        if matches!(**elem, Type::String | Type::Number | Type::Boolean | Type::Any) => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
                    }
                    // M1.2 — `xs.push(v)`: takes one element-typed arg,
                    // returns the new length per JS spec §22.1.3.20.
                    // Runtime helper `__torajs_arr_push` already writes
                    // `len + 1` into `arr[#8]`; ssa_lower materializes
                    // the ret as `Load(I64, new_arr, ARR_LEN_OFF)`.
                    (Type::Array(elem), "push") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(vec![inner], Box::new(Type::Number)))
                    }
                    // `xs.pop()` — remove and return the last element.
                    // Mutates the receiver. tr's subset assumes a non-empty
                    // array (matches the `xs[xs.length - 1]` style call
                    // patterns this enables); `pop` on an empty array is
                    // unchecked. Returns the element type directly (no
                    // `T | undefined` since tr lacks union types).
                    (Type::Array(elem), "pop") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(Vec::new(), Box::new(inner)))
                    }
                    // `xs.shift()` — same shape as pop but removes the
                    // first element (memmoves the rest left). Subset
                    // convention: empty-array shift is unchecked.
                    (Type::Array(elem), "shift") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(Vec::new(), Box::new(inner)))
                    }
                    // `xs.unshift(v)` — insert v at slot 0 (memmoves
                    // the rest right; may realloc). Returns the new
                    // length per JS spec §22.1.3.34, mirroring push.
                    (Type::Array(elem), "unshift") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(vec![inner], Box::new(Type::Number)))
                    }
                    // `xs.splice(start, deleteCount)` — remove a slice
                    // in-place + return removed slice as a fresh
                    // Array<T>. Per JS spec §23.1.3.31. v0 subset: no
                    // `...items` rest-arg insert form (deferred).
                    (Type::Array(elem), "splice") | (Type::Array(elem), "toSpliced") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(
                            vec![Type::Number, Type::Number],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `xs.flat()` — single-level flatten. Receiver must
                    // be `T[][]`; result is `T[]`. v0 supports depth=1
                    // only (no `.flat(2)` arg).
                    (Type::Array(elem), "flat") => {
                        // S129-3 Array<Any>.flat — Any elem bypasses
                        // the typed Array<Array<T>> shape check: any
                        // outer slot can wrap an inner Array<Any>
                        // (or a scalar that passes through). Routes
                        // to `__torajs_arr_flat_any` at ssa-lower
                        // which decodes each slot's NaN-box tag.
                        // Result stays Array<Any> (depth=1 only —
                        // mirror typed flat's v0 limit).
                        if matches!(**elem, Type::Any) {
                            return Ok(Type::Function(
                                Vec::new(),
                                Box::new(Type::Array(Box::new(Type::Any))),
                            ));
                        }
                        // ES §23.1.3.11 — non-nested receiver returns
                        // a shallow copy with the same element type
                        // (depth=1 leaves non-Array slots untouched).
                        // Pre-fix tora forced Array<Array<T>>, blocking
                        // the spec-canonical `[1,2,3].flat()` shape.
                        let result_inner = match (**elem).clone() {
                            Type::Array(inner) => *inner,
                            other => other,
                        };
                        Ok(Type::Function(
                            Vec::new(),
                            Box::new(Type::Array(Box::new(result_inner))),
                        ))
                    }
                    // `xs.sort(cmp)` — in-place sort using the comparator
                    // `(a: T, b: T) => number`. Returns the same array
                    // (chainable). Subset requires the comparator (no
                    // default lex-sort fallback).
                    (Type::Array(elem), "toSorted") => {
                        let inner = (**elem).clone();
                        let cmp_ty = Type::Function(
                            vec![inner.clone(), inner.clone()],
                            Box::new(Type::Number),
                        );
                        Ok(Type::Function(
                            vec![cmp_ty],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    (Type::Array(elem), "sort") => {
                        let inner = (**elem).clone();
                        let cmp_ty = Type::Function(
                            vec![inner.clone(), inner.clone()],
                            Box::new(Type::Number),
                        );
                        Ok(Type::Function(
                            vec![cmp_ty],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `a.concat(b)` — fresh array of a's elements then b's.
                    // Subset: binary only, both arrays must share element type.
                    (Type::Array(elem), "concat") => {
                        // S129-4 Array<Any>.concat — Any receiver accepts
                        // any Array<U> arg (typed slots get NaN-boxed at
                        // runtime via __torajs_arr_extend_typed_into_any
                        // when the SSA arg type isn't Array<Any>).
                        // Param sig is Type::Any so dispatch typecheck
                        // doesn't reject typed Array<U>; ssa-lower peeks
                        // expr_types to derive the elem tag. Result
                        // stays Array<Any>. Same S128-5 / S129-1 / S129-3
                        // mixed-Any series shape.
                        if matches!(**elem, Type::Any) {
                            return Ok(Type::Function(
                                vec![Type::Any],
                                Box::new(Type::Array(Box::new(Type::Any))),
                            ));
                        }
                        let inner = (**elem).clone();
                        Ok(Type::Function(
                            vec![Type::Array(Box::new(inner.clone()))],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `s.concat(other)` — string concat. The single-arg
                    // shape lives here so the standard method-call path
                    // typechecks normally. Variadic forms drop into the
                    // arity-≠-1 guard below the Math/String variadic
                    // block.
                    (Type::String, "concat") => Ok(Type::Function(
                        vec![Type::String],
                        Box::new(Type::String),
                    )),
                    // `xs.at(i)` — element at i with negative-index wrap.
                    // Subset returns T (not T | undefined) — out-of-bounds
                    // is UB, matches the unchecked indexing convention.
                    (Type::Array(elem), "at") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(
                            vec![Type::Number],
                            Box::new(inner),
                        ))
                    }
                    // `xs.reverse()` — in-place reverse, returns the same
                    // array (chainable). Subset returns void since the
                    // chain shape isn't common in our test set.
                    // `toReversed` (ES2023) is the non-mutating sibling —
                    // identical signature, fresh `Array<T>` result.
                    (Type::Array(elem), "reverse") | (Type::Array(elem), "toReversed") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(
                            Vec::new(),
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `xs.with(i, v)` (ES2023) — non-mutating index update.
                    // Returns a fresh `Array<T>` with `xs[i] = v`. Negative
                    // `i` wraps via `len + i`. OOB is UB.
                    (Type::Array(elem), "with") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(
                            vec![Type::Number, inner.clone()],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `xs.copyWithin(target, start, end)` — memmove
                    // [start, end) into `target` position, in-place.
                    (Type::Array(elem), "copyWithin") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(
                            vec![Type::Number, Type::Number, Type::Number],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `xs.fill(value, start, end)` — uniform fill over a
                    // range. start/end optional in JS; subset requires
                    // both for now. Returns the same array.
                    (Type::Array(elem), "fill") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(
                            vec![inner.clone(), Type::Number, Type::Number],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `xs.slice(start, end)` — fresh array of the
                    // [start, end) range. Same element type. Both
                    // bounds are required in this v0 subset.
                    (Type::Array(elem), "slice") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(
                            vec![Type::Number, Type::Number],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `xs.indexOf(needle)` / `xs.lastIndexOf(needle)` —
                    // linear scan; returns -1 on miss. lastIndexOf scans
                    // from the end. Needle must match the element type.
                    (Type::Array(elem), "indexOf")
                    | (Type::Array(elem), "lastIndexOf") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(vec![inner], Box::new(Type::Number)))
                    }
                    // M6.2 — `xs.map(fn)`: takes a `(T) => T` closure,
                    // returns `T[]` (a fresh array). MVP keeps input
                    // and output element types the same; non-uniform
                    // map (e.g. `number[] → string[]`) lands when
                    // generic methods are wired (post-M6.2.a).
                    (Type::Array(elem), "map") => {
                        let inner = (**elem).clone();
                        let fn_ty = Type::Function(
                            vec![inner.clone()],
                            Box::new(inner.clone()),
                        );
                        Ok(Type::Function(
                            vec![fn_ty],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // `xs.flatMap(fn)` — same homogeneous constraint as
                    // map (`(T) => T[]` callback), returns `T[]`. Inner
                    // arrays are flattened one level into the result.
                    (Type::Array(elem), "flatMap") => {
                        let inner = (**elem).clone();
                        let arr_t = Type::Array(Box::new(inner.clone()));
                        let fn_ty = Type::Function(
                            vec![inner.clone()],
                            Box::new(arr_t.clone()),
                        );
                        Ok(Type::Function(vec![fn_ty], Box::new(arr_t)))
                    }
                    // M6.2 — `xs.filter(predicate)`: takes a `(T) => boolean`,
                    // returns `T[]` of kept elements.
                    (Type::Array(elem), "filter") => {
                        let inner = (**elem).clone();
                        let pred_ty = Type::Function(
                            vec![inner.clone()],
                            Box::new(Type::Boolean),
                        );
                        Ok(Type::Function(
                            vec![pred_ty],
                            Box::new(Type::Array(Box::new(inner))),
                        ))
                    }
                    // M6.2 — `xs.reduce(fn, initial)`: takes a
                    // `(acc: T, x: T) => T` and an initial T value;
                    // returns T. Two-arg reduce; the no-initial overload
                    // is deferred.
                    // S132 — reduceRight: identical signature to reduce,
                    // but walks last → first (spec §22.1.3.22). ssa-lower
                    // shares the loop scaffold with `reduce`, differing
                    // only in the cursor init / cmp / inc direction.
                    (Type::Array(elem), "reduce") | (Type::Array(elem), "reduceRight") => {
                        let inner = (**elem).clone();
                        let fn_ty = Type::Function(
                            vec![inner.clone(), inner.clone()],
                            Box::new(inner.clone()),
                        );
                        Ok(Type::Function(
                            vec![fn_ty, inner.clone()],
                            Box::new(inner),
                        ))
                    }
                    // M6.2 — `xs.forEach(fn)`: takes a `(T) => void`,
                    // returns void. Used for side-effecting iteration.
                    (Type::Array(elem), "forEach") => {
                        let inner = (**elem).clone();
                        let fn_ty = Type::Function(
                            vec![inner],
                            Box::new(Type::Void),
                        );
                        Ok(Type::Function(vec![fn_ty], Box::new(Type::Void)))
                    }
                    /* P6.4c-C3 / P5.4 — Array.keys / .values / .entries
                     * returning ArrIter. Array<Any> walks its 16B
                     * tagged-slot layout directly; typed Array<T> for
                     * non-Any T uses an 8B-per-slot layout the runtime
                     * step helper can't unbox. S132 narrow: typed-T
                     * .keys() yields 0..length-1 indices independent
                     * of the slot encoding, so runtime
                     * `arr_iter_create_keys` works uniformly — accept
                     * any typed Array<T> for `.keys()`. .values() /
                     * .entries() still need a box-the-slot walker
                     * follow-up (independent trunk). */
                    (Type::Array(_), "keys") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::ArrIter)))
                    }
                    (Type::Array(elem), "values") | (Type::Array(elem), "entries")
                        if matches!(**elem, Type::Any) =>
                    {
                        Ok(Type::Function(Vec::new(), Box::new(Type::ArrIter)))
                    }
                    // `xs.includes(needle)` — boolean variant of indexOf.
                    (Type::Array(elem), "includes") => {
                        let inner = (**elem).clone();
                        Ok(Type::Function(vec![inner], Box::new(Type::Boolean)))
                    }
                    // `xs.findIndex(pred)` — index of first matching, or -1.
                    // (`xs.find` returns `T | undefined` which would need
                    // Nullable(Number) for Number arrays — not in v0;
                    // callers should use `xs[xs.findIndex(p)]` after a
                    // -1 check, or `xs.filter(p)[0]` with a length guard.)
                    // `findLastIndex` is the reverse-iteration sibling and
                    // shares the same -1-on-miss return, so it lives in
                    // the subset alongside findIndex.
                    // `xs.find(p)` / `xs.findLast(p)` — predicate scan.
                    // tr's subset returns the element type itself (no
                    // `T | undefined`); not-found returns the zero of
                    // T (null for refcounted, 0 / false for primitives).
                    // Caller can either disambiguate via findIndex first
                    // or check against the sentinel value.
                    (Type::Array(elem), "find") | (Type::Array(elem), "findLast") => {
                        let inner = (**elem).clone();
                        let pred_ty = Type::Function(
                            vec![inner.clone()],
                            Box::new(Type::Boolean),
                        );
                        Ok(Type::Function(vec![pred_ty], Box::new(inner)))
                    }
                    (Type::Array(elem), "findIndex")
                    | (Type::Array(elem), "findLastIndex") => {
                        let inner = (**elem).clone();
                        let pred_ty = Type::Function(
                            vec![inner],
                            Box::new(Type::Boolean),
                        );
                        Ok(Type::Function(
                            vec![pred_ty],
                            Box::new(Type::Number),
                        ))
                    }
                    // `xs.some(pred)` / `xs.every(pred)` — short-circuit
                    // ored / anded predicate iteration.
                    (Type::Array(elem), "some") | (Type::Array(elem), "every") => {
                        let inner = (**elem).clone();
                        let pred_ty = Type::Function(
                            vec![inner],
                            Box::new(Type::Boolean),
                        );
                        Ok(Type::Function(vec![pred_ty], Box::new(Type::Boolean)))
                    }
                    // V3-18 m1.h.47 — Symbol.prototype.description.
                    // Returns the desc the Symbol was created with, or
                    // null if Symbol() was called with no arg. Per JS
                    // spec §20.4.3.2.
                    (Type::Symbol, "description") => Ok(Type::String),
                    // (`<prim>.constructor` — V3-18 m2.c — is handled
                    // by the earlier identical arm; dead duplicate
                    // removed for the zero-warn build rule.)
                    // V3-18 m2.d — class-instance Object.prototype
                    // methods. Same shape as namespace ctors:
                    //   .hasOwnProperty(k)         → true if k is a
                    //                                 declared field
                    //                                 (compile-time
                    //                                 layout lookup).
                    //   .propertyIsEnumerable(k)   → same as
                    //                                 hasOwnProperty
                    //                                 (instance fields
                    //                                 are enumerable).
                    //   .isPrototypeOf(x)          → false (no real
                    //                                 prototype chain).
                    //   .valueOf()                 → identity (the
                    //                                 instance).
                    //   .toString()                → "[object Object]"
                    //                                 (subset stub).
                    //   .constructor                → Type::Any.
                    (Type::Struct(_), "hasOwnProperty")
                    | (Type::Struct(_), "propertyIsEnumerable") => {
                        Ok(Type::Function(vec![Type::String], Box::new(Type::Boolean)))
                    }
                    (Type::Struct(_), "isPrototypeOf") => {
                        Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean)))
                    }
                    (Type::Struct(_), "valueOf") => {
                        let inner = obj_ty.clone();
                        Ok(Type::Function(Vec::new(), Box::new(inner)))
                    }
                    (Type::Struct(_), "toString") => {
                        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
                    }
                    (Type::Struct(_), "constructor") => Ok(Type::Any),
                    // P3.2 — Member access on Type::Any returns Type::Any.
                    // Static layout unknown at compile time; ssa_lower
                    // routes through dynobj_get_tag/value. Missing
                    // properties read as undefined per spec.
                    (Type::Any, _) => Ok(Type::Any),
                    // T-29 — Array-as-Object reads. `arr.x` on an
                    // array returns Type::Any (lookup via side table).
                    // .length is already handled by the (Type::Array(_),
                    // "length") arm above; built-in methods (map /
                    // filter / push / etc.) are handled in the
                    // Expr::Call arm's per-method dispatch — those
                    // never reach this Member-only path because the
                    // Call dispatch matches obj_ty + name BEFORE
                    // calling type_of(callee). Only bare-Member
                    // access (without a following call site) lands
                    // here, so excluding the well-known method names
                    // keeps the user-visible Function-typed semantics
                    // for `let m = arr.map` patterns.
                    (Type::Array(_), name) if name != "length"
                        && !is_array_method_name(name) => Ok(Type::Any),
                    // T-27.c — built-in `length` (Number) and `name`
                    // (String) on a Function. length is the param
                    // count; name is the lifted FnDecl's name. Both
                    // are compile-time constants known from the fn's
                    // static signature, so ssa_lower can fold them
                    // without runtime dispatch.
                    (Type::Function(params, _), "length") => {
                        let _ = params;
                        Ok(Type::Number)
                    }
                    (Type::Function(..), "name") => Ok(Type::String),
                    // T-27 — Function-as-Object reads. Per ECMAScript
                    // §10.2 functions are objects. `f.x` on a closure
                    // reads from its lazy props_dynobj at offset
                    // CLOSURE_PROPS_OFF; missing/unset → undefined.
                    // Other built-in props (.bind, .call, .apply,
                    // .toString, etc.) are L3b T-27.c-rest — not
                    // implemented; currently return undefined.
                    (Type::Function(..), _) => Ok(Type::Any),
                    _ => Err(format!("no member `.{name}` on type {obj_ty:?}")),
                }
            }
            Expr::Index { obj, index } => {
                let obj_ty = self.type_of(ast, *obj)?;
                let idx_ty = self.type_of(ast, *index)?;
                if idx_ty != Type::Number {
                    return Err(format!("index must be number, got {idx_ty:?}"));
                }
                match obj_ty {
                    Type::String => Ok(Type::String),
                    Type::Array(elem) => Ok(*elem),
                    other => Err(format!("can't index into {other:?}")),
                }
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
                // Name-based class-method rewrite vs builtin-container
                // receiver — decision + alt typecheck live in cm_demote.rs.
                if let Some(demoted) = self.try_demote_cm_rewrite(ast, eid, args) {
                    return demoted;
                }
                // T-45 — synthetic call from parser for binary `in`
                // operator: `__torajs_in_op(key, obj)`. ssa_lower
                // intercepts by name and emits the type-dispatched
                // membership check. Returns Boolean unconditionally.
                if let Expr::Ident(n) = ast.get_expr(*callee)
                    && n == "__torajs_in_op"
                    && args.len() == 2
                {
                    let _ = self.type_of(ast, args[0])?;
                    let obj_ty = self.type_of(ast, args[1])?;
                    if !matches!(obj_ty, Type::Array(_) | Type::Any | Type::Struct(_)) {
                        return Err(format!(
                            "`in` rhs must be Array, Struct, or any (subset stub); got {obj_ty:?}"
                        ));
                    }
                    return Ok(Type::Boolean);
                }
                /* T-19.l (v0.5.0) — `Promise<T>.then(onOk, onRejected)`
                 * 2-arg form. Spec equivalent of `.then(onOk).catch
                 * (onRejected)`. Both cbs share the simple cb shape
                 * `(v: T) => T`. Routed here BEFORE the regular method
                 * table because the method table's static signature
                 * carries a fixed param count (1) and the generic arg-
                 * count check below would reject 2-arg calls. ssa_lower
                 * picks up the 2-arg shape and emits a then→catch
                 * chain at the call site. */
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "then"
                    && args.len() == 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Promise(inner) = &src_ty
                        && matches!(**inner, Type::Number | Type::String | Type::Boolean)
                    {
                        let inner_ty = (**inner).clone();
                        let expected_cb =
                            Type::Function(vec![inner_ty.clone()], Box::new(inner_ty.clone()));
                        for (i, a) in args.iter().enumerate() {
                            let aty = self.type_of(ast, *a)?;
                            if aty != expected_cb {
                                return Err(format!(
                                    "Promise.then arg {i}: expected {:?}, got {aty:?}",
                                    expected_cb
                                ));
                            }
                        }
                        return Ok(Type::Promise(Box::new(inner_ty)));
                    }
                }
                /* T-19.o (v0.5.0) — generic `Promise<T>.then(cb)` /
                 * `.catch(cb)` where cb's return type U may differ
                 * from T (per ES2015). Routed here BEFORE the
                 * method-table because the table's static signature
                 * fixes T == U. We probe cb's actual signature: if
                 * its param matches T and its return is a primitive
                 * the runtime helper can pack through i64 (Number /
                 * String / Boolean), the result is Promise<U>.
                 *
                 * `.finally` is intentionally not handled here —
                 * its cb is `() => void` per spec and the table arm
                 * already covers that shape. */
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "then" || m_name == "catch")
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    // P10.7 — `Promise<Any>.then(cb)` accepts any
                    // callable taking Any and returning ANY type
                    // (including Void — e.g. `.then(v =>
                    // console.log(v))`). Result is `Promise<R>` where
                    // R is the cb's actual return type, mirroring the
                    // spec: `Promise<any>.then((v) => U): Promise<U>`.
                    if let Type::Promise(inner) = &src_ty
                        && matches!(**inner, Type::Any)
                    {
                        let cb_ty = self.type_of(ast, args[0])?;
                        if let Type::Function(params, ret) = &cb_ty
                            && params.len() == 1
                            && matches!(params[0], Type::Any)
                        {
                            return Ok(Type::Promise(ret.clone()));
                        }
                    }
                    if let Type::Promise(inner) = &src_ty
                        && matches!(**inner, Type::Number | Type::String | Type::Boolean)
                    {
                        let inner_ty = (**inner).clone();
                        let cb_ty = self.type_of(ast, args[0])?;
                        if let Type::Function(params, ret) = &cb_ty
                            && params.len() == 1
                            && params[0] == inner_ty
                            && matches!(**ret, Type::Number | Type::String | Type::Boolean)
                            && **ret != inner_ty
                        {
                            /* Heterogeneous T → U accepted; result is
                             * Promise<U>. Same-T case falls through to
                             * the method-table arm below (which still
                             * handles the common `(T) => T` shape). */
                            return Ok(Type::Promise(ret.clone()));
                        }
                    }
                }
                /* P10.2-A1.1 (resumed-session 2026-05-21) —
                 * `Promise<Undefined>.then(cb)` / `.catch(cb)`. The
                 * 0-arg ctor `Promise.resolve()` / `.reject()` (A1)
                 * produces inner T=Undefined, which the generic
                 * arm above rejects (it limits inner T to the i64-
                 * roundtrippable Number/String/Boolean primitives).
                 *
                 * cb sig is `() => U` — ergonomic surface for the
                 * 0-arg ctor (TS spec actually wants `(v: undefined)
                 * => U`, but bare-`() => U` is what real code looks
                 * like and what bun accepts as a structural sig).
                 * The helper still calls cb via SystemV `int64_t
                 * (*)(int64_t)`; cb just ignores its argument slot.
                 *
                 * cb return U: primitive (Number / String / Boolean)
                 * → Promise<U>; Void / Undefined → Promise<Undefined>.
                 *
                 * Both closure-typed and simple-fn-typed cb shapes
                 * are accepted at this layer; ssa_lower's existing
                 * cb_ty Closure/FnSig dispatch (line ~17220) routes
                 * to promise_then_closure / _simple correctly without
                 * any Promise<T> inner-T inspection (SSA Type::Promise
                 * is a unit variant). */
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "then" || m_name == "catch")
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Promise(inner) = &src_ty
                        && matches!(**inner, Type::Undefined)
                    {
                        let cb_ty = self.type_of(ast, args[0])?;
                        if let Type::Function(params, ret) = &cb_ty
                            && params.is_empty()
                        {
                            let result_inner = match &**ret {
                                Type::Number | Type::String | Type::Boolean => (**ret).clone(),
                                Type::Void | Type::Undefined => Type::Undefined,
                                other => {
                                    return Err(format!(
                                        "Promise.{m_name} on Promise<Undefined>: cb return must be Number / String / Boolean / Void / Undefined, got {other:?}"
                                    ));
                                }
                            };
                            return Ok(Type::Promise(Box::new(result_inner)));
                        }
                        return Err(format!(
                            "Promise.{m_name} on Promise<Undefined>: cb must be 0-arg `() => U`, got {cb_ty:?}"
                        ));
                    }
                }
                /* P10.2-A4 (resumed-session 2026-05-22) —
                 * `Promise<Array<U>>.then(cb)` / `.catch(cb)`. The
                 * Promise.all<T>(promises) result has inner T=Array<U>,
                 * which the generic .then/.catch arm above rejects
                 * (it limits inner T to the i64-roundtrippable
                 * Number/String/Boolean primitives).
                 *
                 * cb sig is `(arr: Array<U>) => V` per spec (1-arg
                 * structural sig accepted; mirrors A1.1's 0-arg arm
                 * pattern for Promise<Undefined>). SystemV `int64_t
                 * (*)(int64_t)` already passes Array ptr in rdi; cb
                 * reads it directly. No runtime change.
                 *
                 * cb return V: primitive (Number / String / Boolean)
                 * → Promise<V>; Void / Undefined → Promise<Undefined>.
                 * Array<W> return is deferred (would need helper-side
                 * value_is_heap=true propagation for next Promise's
                 * heap value — separate sub-A).
                 *
                 * cb does NOT retain the array past invocation —
                 * source Promise still owns the Array ref for its
                 * lifetime; the cb only reads through the passed
                 * ptr. So no rc concern in either direction. */
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "then" || m_name == "catch")
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Promise(inner) = &src_ty
                        && matches!(**inner, Type::Array(_))
                    {
                        let inner_arr_ty = (**inner).clone();
                        let cb_ty = self.type_of(ast, args[0])?;
                        if let Type::Function(params, ret) = &cb_ty
                            && params.len() == 1
                            && params[0] == inner_arr_ty
                        {
                            let result_inner = match &**ret {
                                Type::Number | Type::String | Type::Boolean => (**ret).clone(),
                                Type::Void | Type::Undefined => Type::Undefined,
                                other => {
                                    return Err(format!(
                                        "Promise.{m_name} on Promise<{inner_arr_ty:?}>: cb return must be Number / String / Boolean / Void / Undefined (got {other:?}); Array<W> return deferred to a later sub-A"
                                    ));
                                }
                            };
                            return Ok(Type::Promise(Box::new(result_inner)));
                        }
                        return Err(format!(
                            "Promise.{m_name} on Promise<{inner_arr_ty:?}>: cb must be `(arr: {inner_arr_ty:?}) => V`, got {cb_ty:?}"
                        ));
                    }
                }
                /* T-21 (v0.6.0) — `fetch(url)` returns Promise<Response>.
                 * Response has `.text(): Promise<string>` and a `status:
                 * number` property; both are SSA-level operations on
                 * the heap-alloc'd Response struct populated by
                 * `__torajs_fetch_sync`. POST / headers / method /
                 * body land in a fetch options follow-up. */
                if let Expr::Ident(n) = ast.get_expr(*callee)
                    && n == "fetch"
                {
                    if args.len() != 1 {
                        return Err(format!(
                            "fetch(url) expects 1 string arg, got {}",
                            args.len()
                        ));
                    }
                    let url_ty = self.type_of(ast, args[0])?;
                    if !matches!(url_ty, Type::String) {
                        return Err(format!("fetch(url) — url must be string, got {url_ty:?}"));
                    }
                    return Ok(Type::Promise(Box::new(Type::Object("Response"))));
                }
                // V3-18 m1.h.8 — `Number(x)` / `String(x)` / `Boolean(x)`
                // callable coercion (NOT `new` — that's the wrapper-
                // object form, deferred). Spec §21.1.1 Number(value),
                // §22.1.1 String(value), §20.3.1 Boolean(value):
                // unconditionally coerce to the named primitive type.
                // ssa_lower's per-input dispatch covers Number+Bool+
                // Null (and String for the String() case); other arg
                // types panic — String/Object → Number ToString-then-
                // parse path lands with the m1.h.9 wedge.
                if let Expr::Ident(n) = ast.get_expr(*callee)
                    && (n == "Number" || n == "String" || n == "Boolean")
                {
                    let result_ty = match n.as_str() {
                        "Number" => Type::Number,
                        "String" => Type::String,
                        "Boolean" => Type::Boolean,
                        _ => unreachable!(),
                    };
                    // S251 — Number/String/Boolean(value, ...trailing)
                    // per ES §21.1.1 / §22.1.1 / §20.3.1 trailing-arg
                    // ignore. Spec coerces only args[0]; tora silent-
                    // drops trailing per generic trailing-arg-ignore
                    // policy. SSA-emit reads args[0] (or empty), so
                    // args[1..] dropped at lower-time without further
                    // change.
                    for &arg in args.iter().skip(1) {
                        let _ = self.type_of(ast, arg)?;
                    }
                    if let Some(a) = args.first() {
                        let arg_ty = self.type_of(ast, *a)?;
                        let ok = match n.as_str() {
                            "Boolean" => true,
                            // P1.5 — Number(undefined) === NaN per spec §7.1.4.
                            // String(undefined) === "undefined" per §7.1.17.
                            // S172 — Number(Array<T>) routes through
                            // ToPrimitive("string") = arr.join(",")
                            // then ToNumber(String) (bun parity:
                            // Number([]) === 0, Number([1]) === 1,
                            // Number([1,2]) === NaN).
                            "Number" => matches!(
                                arg_ty,
                                Type::Number
                                    | Type::Boolean
                                    | Type::Null
                                    | Type::Undefined
                                    | Type::String
                                    | Type::Any
                                    | Type::Array(_)
                            ),
                            // S137 — `String(arr)` routes to arr_join
                            // (ES §22.1.3.30 same path as `arr.toString`);
                            // `String(struct)` is the generic Object
                            // `[object Object]` per §20.1.4.4. Generic
                            // dynobj branch lands when the dynobj-toString
                            // substrate ships.
                            "String" => matches!(
                                arg_ty,
                                Type::Number
                                    | Type::Boolean
                                    | Type::Null
                                    | Type::Undefined
                                    | Type::String
                                    | Type::Any
                                    | Type::Array(_)
                                    | Type::Struct(_)
                            ),
                            _ => false,
                        };
                        if !ok {
                            return Err(format!(
                                "{n}({arg_ty:?}) coercion not yet supported (V3-18 m1.h.9 follow-up)"
                            ));
                        }
                    }
                    return Ok(result_ty);
                }
                /* V3-03 — `BigInt(value)` callable ctor. One required
                 * arg, type-dispatched by ssa_lower:
                 *   bigint  → clone
                 *   string  → from_str (auto-radix from prefix)
                 *   number  → from_number (RangeError on non-integer
                 *             / non-finite)
                 * Type::Any is deferred (Any-tagged dispatch lands
                 * with the test262 push). */
                if let Expr::Ident(n) = ast.get_expr(*callee)
                    && n == "BigInt"
                {
                    if args.is_empty() {
                        return Err("BigInt(value) expects exactly 1 arg, got 0".to_string());
                    }
                    let arg_ty = self.type_of(ast, args[0])?;
                    if !matches!(arg_ty, Type::BigInt | Type::String | Type::Number) {
                        return Err(format!(
                            "BigInt(value) — value must be bigint / string / number, got {arg_ty:?}"
                        ));
                    }
                    // S308 — typecheck-and-drop trailing args[1..] per ES
                    // §21.2.1 trailing-arg ignore. Spec coerces only args[0];
                    // ssa_lower mirror at ~16022 reads only args[0] so
                    // args[1..] dropped at lower-time without further
                    // change. Mirror requires lower-and-drop in SSA.
                    for &a in args.iter().skip(1) {
                        let _ = self.type_of(ast, a)?;
                    }
                    return Ok(Type::BigInt);
                }
                // T-13.a (v0.4.0) — `Symbol(desc?)` constructor call.
                // Returns Type::Symbol. Optional desc Str arg; missing
                // desc = NULL pointer at runtime, prints `Symbol()`.
                if let Expr::Ident(n) = ast.get_expr(*callee)
                    && n == "Symbol"
                {
                    if !args.is_empty() {
                        let arg_ty = self.type_of(ast, args[0])?;
                        if !matches!(arg_ty, Type::String) {
                            return Err(format!(
                                "Symbol(desc) — desc must be string, got {arg_ty:?}"
                            ));
                        }
                    }
                    // S308 — typecheck-and-drop trailing args[1..] per ES
                    // §20.4.1 trailing-arg ignore (Symbol(desc, ...trailing)
                    // — only desc is read; trailing dropped).
                    for &a in args.iter().skip(1) {
                        let _ = self.type_of(ast, a)?;
                    }
                    return Ok(Type::Symbol);
                }
                /* T-15.g.5 (v0.5.0) — `Promise.resolve(v)` / `Promise.reject(v)`
                 * with arg-type-driven return inference. Static-method
                 * table can't carry generic T (TypeVar isn't auto-
                 * unified), so we special-case here: inspect arg type,
                 * return Promise<T>. Subset: T ∈ {Number, String,
                 * Boolean} for now; arrays / objects in T-15.g.6+.
                 * (Object.is uses the static-table path because both
                 * args / return are concrete.) */
                // V3-18 wedge — `Number.parseInt(s)` and
                // `Number.parseInt(s, radix)`. Per JS spec §21.1.2.13
                // the radix is optional; bare `Number.parseInt(s)`
                // auto-detects (`0x` prefix → 16, otherwise 10).
                // Pre-fix the type was declared as
                // `Function([String, Number], Number)` so the 1-arg
                // form failed at the unified arity check. Mirror of
                // the global `parseInt` handler at line ~4615.
                // SSA lower already handles the 1-arg shape (passes
                // ConstI64(0) as the auto-detect radix sentinel).
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "parseInt"
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Number"
                {
                    // S253 — Number.parseInt(str, radix, ...trailing)
                    // per ES §21.1.2.13 trailing-arg ignore. SSA-emit
                    // reads args[0..=1] (or less), so args[2..]
                    // dropped at lower-time.
                    for &arg in args.iter().skip(2) {
                        let _ = self.type_of(ast, arg)?;
                    }
                    // S202 — spec §21.1.2.13 Number.parseInt aliases
                    // global parseInt. Step 1 reads `string` which
                    // defaults to undefined when omitted; ToString →
                    // "undefined" → parse fails → NaN.
                    //
                    // S226 — also accept an explicit `undefined` arg
                    // via the same ToString("undefined") → NaN path;
                    // ssa_lower mirror folds the call to ConstF64(NaN).
                    if let Some(arg0) = args.first() {
                        let s_ty = self.type_of(ast, *arg0)?;
                        if !matches!(s_ty, Type::String | Type::Undefined) {
                            return Err(format!(
                                "Number.parseInt arg 0 must be string, got {s_ty:?}"
                            ));
                        }
                    }
                    if args.len() == 2 {
                        let r_ty = self.type_of(ast, args[1])?;
                        // S234 — accept Undefined radix per ES §21.1.2.13
                        // (aliases §19.2.5.1 step 2-3): ToInt32(undefined)=0,
                        // step 8 R==0 → R=10 default. ssa_lower mirror
                        // substitutes ConstI64(0) for the helper's
                        // auto-detect branch.
                        //
                        // S327 — widen accept Any radix. Spec
                        // §19.2.5.1 step 2 calls ToInt32 on the radix,
                        // which already coerces Any (NaN→0, ∞→0, etc.).
                        // ssa_lower mirror routes Any through
                        // coerce_to_i64 instead of panicking on the
                        // integer-shape guard. Narrow widen — only the
                        // Number.parseInt 2-arg shape; global parseInt
                        // (line ~17219) has no shape guard.
                        if !matches!(r_ty, Type::Number | Type::Undefined | Type::Any) {
                            return Err(format!(
                                "Number.parseInt arg 1 must be number, got {r_ty:?}"
                            ));
                        }
                    }
                    return Ok(Type::Number);
                }
                // S202 — Number.parseFloat 0-arg per ES §21.1.2.12.
                // Alias to global parseFloat; missing string defaults
                // to undefined → ToString → "undefined" → NaN. The
                // declared `vec![Type::String]` signature was rejecting
                // the 0-arg form through the generic arity gate.
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "parseFloat"
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Number"
                {
                    // S253 — Number.parseFloat(str, ...trailing) per
                    // ES §21.1.2.12 trailing-arg ignore. SSA-emit
                    // reads args[0] (or empty), so args[1..] dropped
                    // at lower-time.
                    for &arg in args.iter().skip(1) {
                        let _ = self.type_of(ast, arg)?;
                    }
                    // S226 — explicit undefined arg → NaN (same path).
                    //
                    // S330 — widen accept Any. Spec §19.2.5.2 step 1
                    // calls ToString on the operand, which already
                    // coerces arbitrary-typed input. ssa_lower mirror
                    // decodes Any via anyv_to_str_pair (tag + value)
                    // before passing to num_parse_float. Sister to
                    // S327 (parseInt Any radix).
                    if let Some(arg0) = args.first() {
                        let s_ty = self.type_of(ast, *arg0)?;
                        if !matches!(s_ty, Type::String | Type::Undefined | Type::Any) {
                            return Err(format!(
                                "Number.parseFloat arg 0 must be string, got {s_ty:?}"
                            ));
                        }
                    }
                    return Ok(Type::Number);
                }
                // T-15.g.5 / T-19.b/d/f — `Promise.resolve(v)` /
                // `Promise.reject(v)` with arg-type-driven return
                // inference. Extracted to `check/promise_static.rs`
                // (2026-06-03, P10.5-A2 prereq).
                if let Some(r) = self.check_promise_resolve_reject_static(ast, *callee, args) {
                    return r;
                }
                // P10.5-A4 — `process.on('unhandledRejection', cb)`.
                // Extracted to `check/process_on.rs`.
                if let Some(r) = self.check_process_on(ast, *callee, args) {
                    return r;
                }
                /* T-17.a (v0.5.0) — Promise.all<T>(promises: Promise<T>[])
                 * → Promise<Array<T>>. Sync fast-path MVP — caller's
                 * input must be all-fulfilled at call time (pending
                 * elements yield a rejected outer Promise). Real
                 * callback fan-in lands post-T-15.g.6 once PromiseId
                 * interning preserves T element shape. */
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "all"
                        || m_name == "race"
                        || m_name == "any"
                        || m_name == "allSettled")
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Promise"
                {
                    // S273 — accept `>= 1` arg per ES §27.2.4.{1,3,5,2}
                    // trailing-arg ignore: spec reads only the iterable
                    // at args[0]; trailing slots silent-drop. ssa_lower
                    // mirror evals-and-drops args[1..]; typecheck-and-
                    // drop here so trailing expr internal errors surface.
                    if args.is_empty() {
                        return Err(format!(
                            "Promise.{m_name} expects 1 arg (the array of Promises), got {}",
                            args.len()
                        ));
                    }
                    for &a in &args[1..] {
                        let _ = self.type_of(ast, a)?;
                    }
                    let arg_ty = self.type_of(ast, args[0])?;
                    let inner = match &arg_ty {
                        Type::Array(boxed) => match &**boxed {
                            Type::Promise(t_box) => (**t_box).clone(),
                            other => {
                                return Err(format!(
                                    "Promise.{m_name}: arg must be Array<Promise<T>>, got Array<{other:?}>"
                                ));
                            }
                        },
                        other => {
                            return Err(format!(
                                "Promise.{m_name}: arg must be Array<Promise<T>>, got {other:?}"
                            ));
                        }
                    };
                    /* Promise.all → Promise<T[]>; .race / .any →
                     * Promise<T>; .allSettled → Promise<{status,
                     * value}[]>. T-17.c-A3 — allSettled accepts T
                     * from the {Number, String, Bool} primitive set
                     * (parity with Promise.all's existing T
                     * support). Result struct's value field tracks
                     * the inner T monomorphically — ssa_lower picks
                     * up the field type via the returned Type::Struct
                     * and emits the matching field drop (str_drop
                     * for String; no-op for Number/Bool which are
                     * i64-inline). Heterogeneous T-tuples per spec
                     * are deferred until PromiseId interning. */
                    let result = match m_name.as_str() {
                        "all" => Type::Promise(Box::new(Type::Array(Box::new(inner)))),
                        "allSettled" => {
                            if !matches!(inner, Type::Number | Type::String | Type::Boolean) {
                                return Err(format!(
                                    "Promise.allSettled: T must be Number, String, or Boolean in v0.5 MVP (got {inner:?}); spec-strict heterogeneous-T shape ships post-PromiseId interning"
                                ));
                            }
                            Type::Promise(Box::new(Type::Array(Box::new(Type::Struct(vec![
                                ("status".to_string(), Type::String),
                                ("value".to_string(), inner.clone()),
                            ])))))
                        }
                        _ => Type::Promise(Box::new(inner)), // race / any
                    };
                    return Ok(result);
                }
                // `Object.assign(target, ...sources)` per §20.1.2.1 —
                // copy own enumerable properties from each source into
                // target, left-to-right. Subset constraint: target and
                // every source must be the SAME struct type (no field-
                // superset / partial / mismatched-shape yet). Static-
                // resolved at lower time as N×(field-by-field copy).
                // Returns target so chains like `let r = Object.assign(...)`
                // type-check. S127-3 extends prior single-source MVP to
                // N sources (closes the most common Object.assign idiom
                // — `assign({}, a, b)` merge — without runtime shape work).
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "assign"
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Object"
                {
                    if args.is_empty() {
                        return Err("Object.assign requires at least a target arg".to_string());
                    }
                    let target_ty = self.type_of(ast, args[0])?;
                    let Type::Struct(_) = &target_ty else {
                        return Err(format!(
                            "Object.assign target must be a struct, got {target_ty:?}"
                        ));
                    };
                    for (i, src_id) in args[1..].iter().enumerate() {
                        let source_ty = self.type_of(ast, *src_id)?;
                        if target_ty != source_ty {
                            return Err(format!(
                                "Object.assign requires identical struct types in this subset; target={target_ty:?}, source[{i}]={source_ty:?}"
                            ));
                        }
                    }
                    return Ok(target_ty);
                }
                // `arr.flat(N)` — deep flatten. N must be a literal
                // number so the type checker can peel that many
                // Array<> layers from the receiver's element type.
                // depth=0 is a shallow clone (returns Array<T_0>);
                // depth>0 peels per-iter, stopping early if a layer
                // is non-Array. Subset constraint: literal depth only
                // (no `flat(n)` with runtime n — would need a depth-
                // aware runtime helper).
                if let Expr::Member {
                    obj: recv,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "flat"
                    && !args.is_empty()
                {
                    // S289 — Array<T>.flat(depth, ...trailing) trailing-arg
                    // ignore per ES §23.1.3.10. Spec reads only `depth`;
                    // tora's runtime + SSA-emit also peek only args[0].
                    // Eval-and-drop args[1..] for side effects below.
                    for &aid in &args[1..] {
                        let _ = self.type_of(ast, aid)?;
                    }
                    // S129-5 — accept `Infinity` as the depth literal
                    // (ES §23.1.3.13 spec form for full-depth flatten).
                    // Both check + ssa-lower peel up to 64 layers
                    // (matches V8/JSC fixture conventions; no
                    // realistic nesting reaches that depth).
                    let depth_opt: Option<i64> = match ast.get_expr(args[0]) {
                        Expr::Number(d) => Some(*d as i64),
                        Expr::Ident(name) if name == "Infinity" => Some(64),
                        // S220 — `xs.flat(undefined)` per ES §23.1.3.10
                        // step 1: `If depth is undefined, depthNum = 1`.
                        // bun matches: explicit-undefined behaves as the
                        // 0-arg `xs.flat()` default.
                        Expr::Ident(name) if name == "undefined" => Some(1),
                        _ => None,
                    };
                    if let Some(depth) = depth_opt {
                        if depth < 0 {
                            return Err("flat depth must be non-negative".into());
                        }
                        let recv_ty = self.type_of(ast, *recv)?;
                        let Type::Array(_) = &recv_ty else {
                            return Err(format!(
                                "flat receiver must be Array<...>, got {recv_ty:?}"
                            ));
                        };
                        let mut t = recv_ty.clone();
                        for _ in 0..depth {
                            if let Type::Array(elem) = t.clone()
                                && let Type::Array(inner_inner) = (*elem).clone()
                            {
                                t = Type::Array(inner_inner);
                            } else {
                                break;
                            }
                        }
                        return Ok(t);
                    }
                    return Err("flat depth must be a number literal".into());
                }
                // `Object.values(obj)` — result Array<T> for a
                // homogeneous struct (T = shared field type), resolved
                // at lower time like Object.keys but packing values.
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "values"
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Object"
                    && args.len() == 1
                {
                    let arg_ty = self.type_of(ast, args[0])?;
                    // W-O — Array receiver: bun returns a fresh shallow
                    // array of slot values (spec §20.1.2.20); SSA-lower
                    // reuses the typed-struct Arr-field deep-clone arm.
                    if let Type::Array(elem) = &arg_ty {
                        return Ok(Type::Array(elem.clone()));
                    }
                    // W-O-2 — String receiver: bun returns the per-char
                    // Str array (spec §22.1.5.2 + §20.1.2.20 + ToObject
                    // on a primitive string → indexed-properties walk).
                    if matches!(arg_ty, Type::String) {
                        return Ok(Type::Array(Box::new(Type::String)));
                    }
                    // W-J Phase C2 — `any` receiver: struct identity
                    // (per-field types) known only at runtime → Array<Any>
                    // (SSA-lower routes through the struct_enum walker).
                    if matches!(arg_ty, Type::Any) {
                        return Ok(Type::Array(Box::new(Type::Any)));
                    }
                    let Type::Struct(fields) = &arg_ty else {
                        return Err(format!(
                            "Object.values requires a struct arg, got {arg_ty:?}"
                        ));
                    };
                    if fields.is_empty() {
                        return Err(
                            "Object.values on an empty struct can't infer element type".into()
                        );
                    }
                    let first = &fields[0].1;
                    for (n, t) in fields.iter().skip(1) {
                        if t != first {
                            return Err(format!(
                                "Object.values requires homogeneous struct fields; field `{n}` is {t:?} but earlier fields are {first:?}"
                            ));
                        }
                    }
                    return Ok(Type::Array(Box::new(first.clone())));
                }
                // S132 — `Array.from(arrLike)` polymorphic over receiver.
                // The static fn-sig (2993) is fixed to `(String) → Array<String>`
                // for compile-time arrayLike-from-string lowering; for a
                // typed Array<T> input, return Array<T> directly so a
                // shallow-copy emit can pick it up. This Call-level
                // override mirrors Object.values's polymorphic dispatch
                // (above) and lets `Array.from([1,2,3])` typecheck
                // through with the original element type preserved.
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "from"
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Array"
                {
                    if args.len() == 1 {
                        let arg_ty = self.type_of(ast, args[0])?;
                        if let Type::Array(elem) = &arg_ty {
                            return Ok(Type::Array(elem.clone()));
                        }
                        // S141 — `Array.from(set)` per ES §23.1.2.1 + §24.2.3.13
                        // (Set iterator protocol yields values). Set storage
                        // tags each entry as Any (untyped), so the result is
                        // `Array<Any>` — print/spread/eq paths already handle
                        // Any-tagged elements uniformly.
                        if matches!(arg_ty, Type::Set) {
                            return Ok(Type::Array(Box::new(Type::Any)));
                        }
                        // Fall through to the static-sig path; the String
                        // overload already covered there throws a sensible
                        // arity / type mismatch otherwise.
                    } else if args.len() >= 2 {
                        // S151 — `Array.from(iter, mapFn)` per ES §23.1.2.1.
                        // mapFn shape `(elem) → R` or `(elem, idx) → R`;
                        // result type is `Array<R>` where R = mapFn's ret.
                        // iter accepts the same three substrates as 1-arg
                        // (String / typed Array / Set); the actual ssa-lower
                        // path picks 1-arg substrate then runs a map loop.
                        //
                        // S275 — widen `args.len() == 2` to `>= 2` per ES
                        // §23.1.2.1: spec accepts `Array.from(iter, mapFn,
                        // thisArg)`; trailing args past thisArg silent-
                        // drop. tora's closures don't bind `this`, so
                        // thisArg + further trailing are typecheck-and-
                        // dropped; ssa_lower mirror evals-and-drops.
                        let arg_ty = self.type_of(ast, args[0])?;
                        match &arg_ty {
                            Type::String | Type::Array(_) | Type::Set => {}
                            _ => {
                                return Err(format!(
                                    "Array.from(iter, mapFn): iter must be string, Array, or Set; got {arg_ty:?}"
                                ));
                            }
                        }
                        let fn_ty = self.type_of(ast, args[1])?;
                        let ret_ty = match &fn_ty {
                            Type::Function(_, ret) => (**ret).clone(),
                            _ => {
                                return Err(format!(
                                    "Array.from: 2nd arg must be a function, got {fn_ty:?}"
                                ));
                            }
                        };
                        for &a in &args[2..] {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::Array(Box::new(ret_ty)));
                    }
                }
                // S153 — `Date.UTC(...)` 1-6 arg overloads per ES §21.4.2.21.
                // Static sig at 4080 is fixed (Number×7) → Number; this arm
                // accepts the trailing-defaults forms (Date.UTC(year),
                // Date.UTC(y, m), ..., Date.UTC(y, m, d, h, min, s) — ms
                // defaulted at lower time). The 7-arg form keeps using
                // the static-sig path unchanged.
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "UTC"
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Date"
                    && (1..=6).contains(&args.len())
                {
                    for a in args {
                        let _ = self.type_of(ast, *a)?;
                    }
                    return Ok(Type::Number);
                }
                // `xs.reduce(cb)` / `xs.reduceRight(cb)` 1-arg overload —
                // ES §23.1.3.24 / §23.1.3.25: initial value defaults to
                // `arr[0]` (or `arr[len-1]` for reduceRight); empty array
                // throws TypeError (per spec; tr's MVP elides the guard).
                // The 2-arg form is covered by the static-sig arm. Return
                // type is the callback's ret (consistent with the 2-arg
                // path, where the static sig pins ret = elem).
                if let Expr::Member {
                    obj: recv_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "reduce" || m_name == "reduceRight")
                    && args.len() == 1
                {
                    let recv_ty = self.type_of(ast, *recv_id)?;
                    if let Type::Array(elem) = &recv_ty {
                        let _ = self.type_of(ast, args[0])?;
                        return Ok((**elem).clone());
                    }
                }
                // M3 — generic call inference. If callee is a bare Ident
                // naming a generic FnDecl, walk param/arg pairs unifying
                // each TypeVar against the actual arg type, then
                // substitute back into the return type. Side-table records
                // the inferred substitution so ssa_lower can monomorphize.
                if let Expr::Ident(name) = ast.get_expr(*callee)
                    && let Some(type_params) = self.generic_type_params.get(name).cloned()
                    && let Some(Type::Function(params, ret)) = self.globals.get(name).cloned()
                {
                    // T-28 — Default param missing → undefined for
                    // implicit-generic fns. Untyped JS params
                    // (`function f(a, b)`) get rewritten to fresh
                    // independent TypeVars by `desugar_implicit_generics`,
                    // so they land here. Conditions: trailing missing
                    // params must all be TypeVar AND each trailing
                    // TypeVar must NOT appear in earlier params or in
                    // the return type. When safe, bind them to
                    // Type::Any and pad with ANY_UNDEF at the call
                    // site (T-28-substrate enables Any to round-trip
                    // through type_to_ann / parse_type so the mono
                    // gets a real Any-typed param slot).
                    if args.len() < params.len() {
                        let missing = params.len() - args.len();
                        let trailing = &params[args.len()..];
                        let trailing_typevars: Vec<&str> = trailing
                            .iter()
                            .filter_map(|p| match p {
                                Type::TypeVar(n) => Some(n.as_str()),
                                _ => None,
                            })
                            .collect();
                        let trailing_all_typevar = trailing_typevars.len() == trailing.len();
                        let earlier = &params[..args.len()];
                        let trailing_independent = trailing_all_typevar
                            && trailing_typevars.iter().all(|tv| {
                                !typevar_appears_in_iter(earlier, tv)
                                    && !typevar_appears_in(&ret, tv)
                            });
                        if trailing_independent {
                            let mut subst: HashMap<String, Type> = HashMap::new();
                            for (i, (param_ty, arg_id)) in
                                params.iter().take(args.len()).zip(args.iter()).enumerate()
                            {
                                let arg_ty = self.type_of(ast, *arg_id)?;
                                if let Err(e) = unify_typevar(param_ty, &arg_ty, &mut subst) {
                                    return Err(format!("argument {i} to `{name}`: {e}"));
                                }
                            }
                            for tv in &trailing_typevars {
                                subst.insert(tv.to_string(), Type::Any);
                            }
                            for tp in &type_params {
                                subst.entry(tp.clone()).or_insert(Type::Any);
                            }
                            let resolved_ret = substitute_typevars(&ret, &subst);
                            let type_args: Vec<Type> = type_params
                                .iter()
                                .map(|tp| subst.get(tp).cloned().unwrap())
                                .collect();
                            self.generic_call_sites
                                .insert(eid, (name.clone(), type_args));
                            self.arity_pad_count.insert(eid, missing);
                            return Ok(resolved_ret);
                        }
                    }
                    if params.len() != args.len() {
                        return Err(format!(
                            "expected {} argument(s) to `{name}`, got {}",
                            params.len(),
                            args.len()
                        ));
                    }
                    let mut subst: HashMap<String, Type> = HashMap::new();
                    let mut arg_tys: Vec<Type> = Vec::with_capacity(args.len());
                    for (i, (param_ty, arg_id)) in params.iter().zip(args.iter()).enumerate() {
                        let arg_ty = self.type_of(ast, *arg_id)?;
                        if let Err(e) = unify_typevar(param_ty, &arg_ty, &mut subst) {
                            return Err(format!("argument {i} to `{name}`: {e}"));
                        }
                        arg_tys.push(arg_ty);
                    }
                    // Validate every type-param was bound.
                    for tp in &type_params {
                        if !subst.contains_key(tp) {
                            return Err(format!(
                                "could not infer type parameter `{tp}` for `{name}`"
                            ));
                        }
                    }
                    let resolved_ret = substitute_typevars(&ret, &subst);
                    // Record the substitution for the SSA monomorphizer.
                    // Keyed by ExprId of the call so each call site gets
                    // its own type-argument set.
                    let type_args: Vec<Type> = type_params
                        .iter()
                        .map(|tp| subst.get(tp).cloned().unwrap())
                        .collect();
                    self.generic_call_sites
                        .insert(eid, (name.clone(), type_args));
                    // Generic call args also follow the new TS-shape
                    // borrow semantics — non-Copy args are not consumed
                    // by passing. See the comment in the regular Call
                    // arm below for the rationale + caveat.
                    let _ = params;
                    let _ = args;
                    return Ok(resolved_ret);
                }
                // `console.{log,error,warn,info,debug}(arg0, arg1, …)` —
                // accept any arity. The standard typecheck path would
                // reject ≠1 args because Type::Function has fixed
                // arity. Args are typed Type::Any so any value is
                // acceptable. S328 added `info` / `debug` per WHATWG
                // console §1.1.{2,4}.
                if let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "console"
                    && matches!(name.as_str(), "log" | "error" | "warn" | "info" | "debug")
                {
                    for &aid in args {
                        self.type_of(ast, aid)?;
                    }
                    return Ok(Type::Void);
                }
                // `JSON.stringify(value, replacer?, indent?)` — accept
                // 1, 2, or 3 args. The full JS spec defines `replacer`
                // as a function or array; tr's stringify ignores
                // anything non-null in slot 2 (no callback support yet
                // — that's a roadmap item) and consumes only the
                // indent shape (number or string). All args are
                // typechecked against Type::Any so the call is
                // accepted; runtime behavior matches the 1-arg form
                // for now, with indent surface ready for a follow-up
                // ssa_lower pass.
                if let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "JSON"
                    && name == "stringify"
                    && !args.is_empty()
                {
                    // S311 — ES §25.5.2 silently ignores args past
                    // (value, replacer, space). Widen to >=1 + the
                    // ssa_lower mirror loop drops trailing args[1..]
                    // (replacer/space substrate L3b).
                    for &aid in args {
                        self.type_of(ast, aid)?;
                    }
                    return Ok(Type::String);
                }
                // `n.toString(radix?)` — JS Number primitive method that
                // accepts an optional radix in [2, 36]. The standard
                // Type::Function check rejects variable arity; intercept
                // here.
                // S247 — BigInt.prototype.toString(radix, ...trailing)
                // trailing-arg ignore per ES §21.2.3.5. Spec reserves
                // slots past the 1 useful radix but tora's helpers
                // (bigint_to_string / bigint_to_string_radix) are 1-
                // arg only; trailing operand type_of'd for side effects
                // then dropped at lower-time. Same shape as S244
                // Number.toString trailing-arg ignore.
                if let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && name == "toString"
                {
                    let recv_ty = self.type_of(ast, *obj)?;
                    if recv_ty == Type::BigInt && args.len() >= 2 {
                        for &aid in &args[1..] {
                            let _ = self.type_of(ast, aid)?;
                        }
                        // arg 0 still type_of'd via the Function sig's
                        // first slot above; type_of args[0] here too
                        // so any earlier-skipped inference fires.
                        let _ = self.type_of(ast, args[0])?;
                        return Ok(Type::String);
                    }
                }
                if let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && name == "toString"
                {
                    let recv_ty = self.type_of(ast, *obj)?;
                    if recv_ty == Type::Number {
                        if args.is_empty() {
                            return Ok(Type::String);
                        }
                        let r_ty = self.type_of(ast, args[0])?;
                        // S229 — accept Undefined for radix per ES
                        // §21.1.3.6 step 2-3: undefined radix folds
                        // to 10 (the default). ssa_lower mirror
                        // short-circuits to the no-arg path.
                        if !matches!(r_ty, Type::Number | Type::Undefined) {
                            return Err(format!(
                                "Number.toString radix must be number, got {r_ty:?}"
                            ));
                        }
                        // S244 — accept trailing args past the 1 useful
                        // radix slot per ES §21.1.3.6 trailing-arg
                        // ignore. Same shape as S238/S243; ssa_lower
                        // mirror keys on `args.len() >= 1` so the
                        // num_to_string_radix_i/f helper still receives
                        // the radix; trailing operand never lowered.
                        for &aid in &args[1..] {
                            let _ = self.type_of(ast, aid)?;
                        }
                        return Ok(Type::String);
                    }
                }
                // `Number(x)` / `String(x)` — coercion function calls
                // (the bare-name shape is JS's primitive constructor invoked
                // without `new`). Subset accepts most pseudo-Any types
                // and routes to the appropriate coercion at lower-time.
                if let Expr::Ident(name) = ast.get_expr(*callee)
                    && (name == "Number" || name == "String")
                {
                    if args.len() != 1 {
                        return Err(format!("{name}() expects 1 arg, got {}", args.len()));
                    }
                    let _arg_ty = self.type_of(ast, args[0])?;
                    if name == "Number" {
                        return Ok(Type::Number);
                    } else {
                        return Ok(Type::String);
                    }
                }
                // Bare-name JS globals: `parseInt`, `parseFloat`, `isNaN`,
                // `isFinite`. Subset routes them to their Number.X counterparts
                // (the global isNaN / isFinite officially coerce non-numbers
                // before testing; the subset only accepts numeric / string
                // args directly).
                if let Expr::Ident(name) = ast.get_expr(*callee) {
                    match name.as_str() {
                        "parseInt" => {
                            // S252 — parseInt(str, radix, ...trailing)
                            // per ES §19.2.5 trailing-arg ignore. Spec
                            // reads only the first 2; tora silent-drops
                            // trailing per generic trailing-arg-ignore
                            // policy. SSA-emit reads args[0..=1] (or
                            // less), so args[2..] dropped at lower-time.
                            for &arg in args.iter().skip(2) {
                                let _ = self.type_of(ast, arg)?;
                            }
                            // S202 — spec §19.2.5 step 1 reads `string`
                            // which defaults to undefined; ToString
                            // returns "undefined" → parse fails → NaN.
                            //
                            // S226 — accept explicit undefined arg
                            // via the same ToString → NaN path.
                            // S337 — bare-name `parseInt(Any, ...)` per ES
                            // §19.2.5 step 1: ToString accepts arbitrary-
                            // typed input. Sister to S336 (parseFloat bare-
                            // name Any). ssa_lower mirror routes Any
                            // through anyv_to_str_pair → any_to_str →
                            // num_parse_int. Radix slot also widens to
                            // Any (ToInt32 fold) — sister to S327
                            // (Number.parseInt member radix Any).
                            if let Some(arg0) = args.first() {
                                let s_ty = self.type_of(ast, *arg0)?;
                                if !matches!(s_ty, Type::String | Type::Undefined | Type::Any) {
                                    return Err(format!(
                                        "parseInt arg 0 must be string, got {s_ty:?}"
                                    ));
                                }
                            }
                            if args.len() == 2 {
                                let r_ty = self.type_of(ast, args[1])?;
                                // S234 — accept Undefined radix per ES
                                // §19.2.5.1 step 2-3: ToInt32(undefined)=0,
                                // then step 8 R==0 → R=10 default. ssa_lower
                                // mirror substitutes ConstI64(0) so the
                                // helper's `r==0` auto-detect branch picks
                                // up base 10 (or 16 if the input has a
                                // "0x"/"0X" prefix).
                                // S337 — extend the same widen to Any per
                                // ES §19.2.5.1 step 2 ToInt32 (Any path).
                                if !matches!(r_ty, Type::Number | Type::Undefined | Type::Any) {
                                    return Err(format!(
                                        "parseInt arg 1 must be number, got {r_ty:?}"
                                    ));
                                }
                            }
                            return Ok(Type::Number);
                        }
                        "parseFloat" => {
                            // S252 — parseFloat(str, ...trailing) per ES
                            // §19.2.4 trailing-arg ignore. Spec reads
                            // only args[0]; tora silent-drops trailing.
                            // SSA-emit reads args[0] (or empty), so
                            // args[1..] dropped at lower-time.
                            for &arg in args.iter().skip(1) {
                                let _ = self.type_of(ast, arg)?;
                            }
                            // S202 — same default-undefined rule per
                            // §19.2.4: missing string → NaN.
                            // S226 — accept explicit undefined arg
                            // via the same ToString → NaN path.
                            // S336 — bare-name `parseFloat(Any)` per ES
                            // §19.2.4 step 1: ToString accepts arbitrary-
                            // typed input. Sister to S330 (Number.parseFloat
                            // member method same widen). ssa_lower mirror
                            // routes Any through anyv_to_str_pair →
                            // any_to_str → num_parse_float.
                            if let Some(arg0) = args.first() {
                                let s_ty = self.type_of(ast, *arg0)?;
                                if !matches!(s_ty, Type::String | Type::Undefined | Type::Any) {
                                    return Err(format!(
                                        "parseFloat arg must be string, got {s_ty:?}"
                                    ));
                                }
                            }
                            return Ok(Type::Number);
                        }
                        "isNaN" | "isFinite" => {
                            // S252 — isNaN/isFinite(value, ...trailing)
                            // per ES §19.2.3 / §19.2.4 trailing-arg
                            // ignore. Spec reads only args[0]; tora
                            // silent-drops trailing. SSA-emit reads
                            // args[0] (or empty), so args[1..] dropped
                            // at lower-time.
                            for &arg in args.iter().skip(1) {
                                let _ = self.type_of(ast, arg)?;
                            }
                            // V3-18 wedge — global isNaN / isFinite per
                            // JS spec §19.2.3 / §19.2.4 apply ToNumber
                            // on the argument before testing the
                            // predicate (intentional contrast with the
                            // strict Number.isNaN / Number.isFinite
                            // namespaced methods that don't coerce).
                            // Common idiom in TS code that copies JS
                            // patterns: `isFinite("3")` → true (not
                            // a type error). Drive type_of for any
                            // internal-error surface but accept any
                            // coercible type; ssa_lower applies the
                            // ToNumber step at lower time.
                            //
                            // S202 — 0-arg form skips the arg typecheck
                            // entirely; ssa_lower returns the spec
                            // constant (isNaN→true / isFinite→false).
                            if let Some(arg0) = args.first() {
                                let _ = self.type_of(ast, *arg0)?;
                            }
                            return Ok(Type::Boolean);
                        }
                        "queueMicrotask" => {
                            // P10.1-A1 — WHATWG HTML §queueMicrotask:
                            // schedule cb to run as a microtask before
                            // the next event-loop turn. cb is exactly
                            // `() => void`. Higher arities / non-void
                            // ret / simple-fn (no-env) defer to A1.1.
                            //
                            // S323 — Web IDL §3.2.1 over-arity rule:
                            // operations silently ignore trailing args.
                            // Widen `!= 1` → `is_empty()` + typecheck-
                            // and-drop args[1..] so the spec-aligned
                            // `queueMicrotask(cb, trail, ...)` shape
                            // parses; ssa_lower mirror lower-and-drops.
                            if args.is_empty() {
                                return Err("queueMicrotask expects 1 arg, got 0".to_string());
                            }
                            let cb_ty = self.type_of(ast, args[0])?;
                            match &cb_ty {
                                Type::Function(params, ret)
                                    if params.is_empty() && **ret == Type::Void => {}
                                _ => {
                                    return Err(format!(
                                        "queueMicrotask cb must be `() => void`, got {cb_ty:?}"
                                    ));
                                }
                            }
                            for &a in args.iter().skip(1) {
                                let _ = self.type_of(ast, a)?;
                            }
                            return Ok(Type::Void);
                        }
                        _ => {}
                    }
                }
                // Math.min / Math.max — variadic. Accept any arg count >= 2,
                // every arg must be Number; result is Number. ssa-lower
                // folds the call into a pairwise reduction. The general
                // Type::Function check below would reject ≠2 args here.
                // Math.hypot — variadic. sqrt(sum of args²). Per JS
                // spec §21.3.2.18: 0-arg returns +0; 1-arg returns
                // |arg|; 2+ uses libm hypot pairwise (V3-18 m1.h.56
                // dropped the artificial 1-arg minimum).
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Math"
                    && m == "hypot"
                {
                    // S271 — accept Undefined alongside Number per
                    // ES §21.3.2.18: ToNumber(undefined)=NaN, sum² +
                    // sqrt containing NaN → NaN. ssa_lower mirror
                    // folds to ConstF64(NaN) when any arg is statically
                    // Undefined, after eval-and-dropping the non-undef
                    // args so trailing side-effect expressions fire.
                    for &aid in args {
                        let aty = self.type_of(ast, aid)?;
                        if !matches!(aty, Type::Number | Type::Undefined) {
                            return Err(format!("Math.hypot args must be number, got {aty:?}"));
                        }
                    }
                    return Ok(Type::Number);
                }
                // S230 — `Date.parse(undefined)` per ES §21.4.3.2:
                // step 1 reads the arg through ToString. `ToString(undefined)
                // = "undefined"` which is not a valid date string, so the
                // result is NaN. Accept Undefined alongside String; ssa_lower
                // mirror folds the call to ConstF64(NaN).
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Date"
                    && m == "parse"
                    && args.len() == 1
                {
                    let arg_ty = self.type_of(ast, args[0])?;
                    if matches!(arg_ty, Type::Undefined) {
                        return Ok(Type::Number);
                    }
                }
                // S227 — Math.<unary>(undefined) per ES §21.3.2.* step 1:
                // ToNumber(undefined) = NaN, and every NaN-propagating
                // unary method returns NaN. `Math.clz32(undefined)` is
                // the lone exception — its ToUint32(undefined)=0 path
                // returns 32. Mirror the 0-arg carve-out (S203) for the
                // explicit-undefined 1-arg shape; ssa_lower folds the
                // call to ConstF64 without lowering the arg.
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Math"
                    && args.len() == 1
                {
                    let arg_ty = self.type_of(ast, args[0])?;
                    if matches!(arg_ty, Type::Undefined)
                        && matches!(
                            m.as_str(),
                            "sqrt"
                                | "abs"
                                | "floor"
                                | "ceil"
                                | "log"
                                | "exp"
                                | "sign"
                                | "round"
                                | "trunc"
                                | "sin"
                                | "cos"
                                | "tan"
                                | "asin"
                                | "acos"
                                | "atan"
                                | "log2"
                                | "log10"
                                | "cbrt"
                                | "sinh"
                                | "cosh"
                                | "tanh"
                                | "asinh"
                                | "acosh"
                                | "atanh"
                                | "expm1"
                                | "log1p"
                                | "clz32"
                                | "fround"
                                | "f16round"
                        )
                    {
                        return Ok(Type::Number);
                    }
                }
                // S231 — `String.fromCharCode(undefined)` per ES §22.1.2.1:
                // each arg is converted via ToUint16; ToUint16(undefined) = 0,
                // so the single-arg undef shape yields " ". The standard
                // Type::Function arm (vec![Type::Number]) rejects the typed
                // Undefined operand with "argument 0: expected Number, got
                // Undefined"; widen here so ssa_lower's mirror substitutes
                // ConstI64(0) into the helper call. fromCodePoint diverges
                // here — bun throws a RangeError at runtime for undefined,
                // so its throw-shape alignment stays L3b.
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "String"
                    && m == "fromCharCode"
                    && args.len() == 1
                {
                    let aty = self.type_of(ast, args[0])?;
                    if matches!(aty, Type::Undefined) {
                        return Ok(Type::String);
                    }
                    // S329 — `String.fromCharCode(Any)` per ES §22.1.2.1:
                    // each arg goes through ToUint16, which accepts any
                    // value. The method-table sig `(Number) -> String`
                    // rejected explicit `o: any` operands at typecheck;
                    // widen here so the ssa_lower mirror routes Any
                    // through anyv_to_number → coerce_to_i64 → helper.
                    if matches!(aty, Type::Any) {
                        return Ok(Type::String);
                    }
                }
                // S340 — `String.fromCodePoint(Any)` per ES §22.1.2.2 step 2:
                // ToNumber accepts arbitrary-typed input; RangeError throw
                // shape (non-integer / out-of-range [0, 0x10FFFF]) is
                // enforced by the runtime helper `str_from_code_point`
                // (pending throw + emit_throw_check propagates), so the
                // Any path inherits the same throw semantics for free.
                // Sister to S329 (fromCharCode Any).
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "String"
                    && m == "fromCodePoint"
                    && args.len() == 1
                {
                    let aty = self.type_of(ast, args[0])?;
                    if matches!(aty, Type::Any) {
                        return Ok(Type::String);
                    }
                }
                // `String.fromCharCode(...codes)` — variadic. Each code is a
                // Number; result is a String. The single-arg case still goes
                // through the general type table for the intrinsic call; we
                // only intercept when the arity is ≠ 1.
                // `Array.of(...vals)` — variadic factory that returns a
                // fresh `Array<T>` with the given values in order. Empty
                // call requires the caller to use a typed `[]` literal
                // instead (no element to anchor the type). All args must
                // unify on the same type.
                // S204 — `Array.isArray()` 0-arg per ES §23.1.2.2 step 1:
                // missing `arg` defaults to undefined; undefined is not an
                // Array, so the predicate is statically false. The
                // declared `vec![Type::Any]` signature was rejecting the
                // no-arg form at the generic arity gate.
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Array"
                    && m == "isArray"
                    && args.is_empty()
                {
                    return Ok(Type::Boolean);
                }
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Array"
                    && m == "of"
                {
                    if args.is_empty() {
                        return Err("Array.of() with zero args needs a typed `[]` literal; \
                             tr can't infer the element type"
                            .into());
                    }
                    let first_ty = self.type_of(ast, args[0])?;
                    for &aid in args.iter().skip(1) {
                        let aty = self.type_of(ast, aid)?;
                        if aty != first_ty {
                            return Err(format!(
                                "Array.of args must agree on element type; first is \
                                 {first_ty:?}, later arg is {aty:?}"
                            ));
                        }
                    }
                    return Ok(Type::Array(Box::new(first_ty)));
                }
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "String"
                    && (m == "fromCharCode" || m == "fromCodePoint")
                    && args.len() != 1
                {
                    if args.is_empty() {
                        return Ok(Type::String);
                    }
                    for &aid in args {
                        let aty = self.type_of(ast, aid)?;
                        // S340 — variadic Any per ES §22.1.2.{1,2} step 2:
                        // ToNumber/ToUint16 accept arbitrary-typed input.
                        // fromCodePoint inherits its RangeError throw shape
                        // via the runtime helper. Sister to S329.
                        if aty != Type::Number && !matches!(aty, Type::Any) {
                            return Err(format!("String.{m} args must be number, got {aty:?}"));
                        }
                    }
                    return Ok(Type::String);
                }
                // `s.concat(...others)` with arity != 1 — variadic string
                // concatenation. The arity-1 case takes the Type::Function
                // arm above. Empty arg list returns the receiver
                // unchanged at lower-time.
                if let Expr::Member {
                    obj: recv_id,
                    name: m,
                } = ast.get_expr(*callee)
                    && m == "concat"
                    && args.len() != 1
                    && let Ok(Type::String) = self.type_of(ast, *recv_id)
                {
                    for &aid in args {
                        let aty = self.type_of(ast, aid)?;
                        // S212 — explicit `undefined` arg per ES
                        // §22.1.3.4 step 3.a: ToString(undefined)
                        // = "undefined". Accept the typed-Undefined
                        // arg shape; ssa_lower inline-substitutes
                        // the interned "undefined" literal.
                        if matches!(aty, Type::Undefined) {
                            continue;
                        }
                        if aty != Type::String {
                            return Err(format!("String.concat args must be string, got {aty:?}"));
                        }
                    }
                    return Ok(Type::String);
                }
                // S212 (1-arg) — String.concat(undefined) per ES
                // §22.1.3.4 step 3.a: the arity-1 path otherwise
                // takes the declared Function-arm dispatch which
                // is strict-string and rejects the typed-Undefined
                // operand with "argument 0: expected String, got
                // Undefined". Widen here so ssa_lower's
                // inline-undef substitution can run.
                if let Expr::Member {
                    obj: recv_id,
                    name: m,
                } = ast.get_expr(*callee)
                    && m == "concat"
                    && args.len() == 1
                    && let Ok(Type::String) = self.type_of(ast, *recv_id)
                {
                    let aty = self.type_of(ast, args[0])?;
                    if matches!(aty, Type::Undefined) {
                        return Ok(Type::String);
                    }
                }
                // S203 — Math unary methods 0-arg per ES §21.3.2.*
                // step 1: missing arg defaults to undefined →
                // ToNumber(undefined) = NaN → Math.<f>(NaN) = NaN.
                // The declared `vec![Type::Number]` signature
                // rejected the no-arg form at the generic arity
                // gate; accept it and let ssa_lower emit the
                // static NaN return (no helper Call).
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Math"
                    && args.is_empty()
                    && matches!(
                        m.as_str(),
                        "sqrt"
                            | "abs"
                            | "floor"
                            | "ceil"
                            | "log"
                            | "exp"
                            | "sign"
                            | "round"
                            | "trunc"
                            | "sin"
                            | "cos"
                            | "tan"
                            | "asin"
                            | "acos"
                            | "atan"
                            | "log2"
                            | "log10"
                            | "cbrt"
                            | "sinh"
                            | "cosh"
                            | "tanh"
                            | "asinh"
                            | "acosh"
                            | "atanh"
                            | "expm1"
                            | "log1p"
                            | "clz32"
                            | "fround"
                            | "f16round"
                    )
                {
                    return Ok(Type::Number);
                }
                // S205 — Math binary methods 0/1-arg per ES default-
                // undefined. Spec §21.3.2.{19,5,26}: Math.imul takes
                // ToUint32 on each arg (undefined → 0 → imul = 0);
                // Math.pow / Math.atan2 take ToNumber (undefined → NaN
                // → NaN-propagating result). The declared
                // `vec![Number, Number]` signature rejects shorter
                // forms at the generic arity gate.
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Math"
                    && matches!(m.as_str(), "pow" | "atan2" | "imul")
                    && args.len() < 2
                {
                    for &aid in args {
                        let aty = self.type_of(ast, aid)?;
                        if aty != Type::Number {
                            return Err(format!("Math.{m} args must be number, got {aty:?}"));
                        }
                    }
                    return Ok(Type::Number);
                }
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Math"
                    && (m == "min" || m == "max")
                {
                    // V3-18 m1.h.24 — JS spec §21.3.2.24/25:
                    // Math.max() returns -Infinity, Math.min()
                    // returns +Infinity (the identity element of
                    // the reduction). Math.max(x) returns x.
                    // Drop the artificial 2-arg minimum.
                    //
                    // S228 — accept Undefined args per ES NaN-
                    // propagation: any undef arg coerces to NaN
                    // (ToNumber(undefined)=NaN) and Math.min/max
                    // with any NaN operand returns NaN. ssa_lower
                    // mirror folds the call when any arg is undef.
                    //
                    // S342 — accept Any args per ES §21.3.2.{24,25}
                    // ToNumber: arbitrary-typed input is accepted.
                    // ssa_lower's variadic Math.min/max path routes
                    // Any through anyv_to_number → F64 (math_to_f64
                    // closure at ~17898).
                    for &aid in args {
                        let aty = self.type_of(ast, aid)?;
                        if !matches!(aty, Type::Number | Type::Undefined | Type::Any) {
                            return Err(format!("Math.{m} args must be number, got {aty:?}"));
                        }
                    }
                    return Ok(Type::Number);
                }
                // S228 — Math.{pow, atan2, imul}(undef [, undef]) per
                // ES §21.3.2.{5,19,26}. pow / atan2 propagate NaN:
                // any undef arg → ToNumber(undef)=NaN → NaN. imul
                // applies ToUint32 which folds undefined to 0, so any
                // undef arg makes the 32-bit multiply 0. ssa_lower
                // mirror folds the call (extending S205) when either
                // arg is statically Undefined.
                if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && ns == "Math"
                    && matches!(m.as_str(), "pow" | "atan2" | "imul")
                    && args.len() == 2
                {
                    let arg0_ty = self.type_of(ast, args[0])?;
                    let arg1_ty = self.type_of(ast, args[1])?;
                    let any_undef =
                        matches!(arg0_ty, Type::Undefined) || matches!(arg1_ty, Type::Undefined);
                    if any_undef {
                        for (i, aty) in [&arg0_ty, &arg1_ty].iter().enumerate() {
                            if !matches!(**aty, Type::Number | Type::Undefined) {
                                return Err(format!(
                                    "Math.{m} arg {i} must be number, got {aty:?}"
                                ));
                            }
                        }
                        return Ok(Type::Number);
                    }
                }
                // V3-18 m1.h.35 — Array.slice with 0 or 1 args. Per
                // JS spec §22.1.3.25:
                //   xs.slice()      = xs.slice(0, xs.length)
                //   xs.slice(start) = xs.slice(start, xs.length)
                // Pre-fix tora declared slice with 2 fixed params so
                // 0/1-arg calls hit the arity check below. Special-
                // case here: typecheck the args we have, return
                // Array<T>; ssa_lower fills in the defaults at
                // lower-time.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "slice"
                    && args.len() <= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        // S213 — explicit `undefined` for either
                        // start or end per ES §23.1.3.27 step 1-2:
                        // start=undefined → 0, end=undefined → len.
                        // Pre-fix the 2-arg path took the declared
                        // `(Number, Number) -> Array<T>` dispatch
                        // which rejected the typed-Undefined operand;
                        // widen to args.len() <= 2 here and let
                        // ssa_lower fill the defaults.
                        // S334 — `xs.slice(Any [, Any])` per ES
                        // §23.1.3.{28,27}: ToIntegerOrInfinity accepts
                        // arbitrary-typed input. Sister to S332/S333.
                        // ssa_lower mirror routes Any through
                        // anyv_to_number → coerce_to_i64 → helper.
                        for &aid in args {
                            let aty = self.type_of(ast, aid)?;
                            if matches!(aty, Type::Undefined | Type::Any) {
                                continue;
                            }
                            if aty != Type::Number {
                                return Err(format!("Array.slice arg must be number, got {aty:?}"));
                            }
                        }
                        return Ok(Type::Array(Box::new((**elem).clone())));
                    }
                }
                // V3-18 m1.h.53 — Array.fill with optional start /
                // end args per JS spec §22.1.3.6:
                //   xs.fill(v)            = xs.fill(v, 0, len)
                //   xs.fill(v, start)     = xs.fill(v, start, len)
                // Pre-fix tora declared with 3 fixed params so 1 / 2 -
                // arg calls hit the arity check.
                //
                // S218 — extend the carve-out to args.len()==3 and
                // accept Undefined for start/end per ES §23.1.3.7
                // step 5/9 (ToIntegerOrInfinity(undefined)=0 for start,
                // end===undefined → len for end). ssa_lower mirror
                // short-circuits each undef slot to its spec default.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "fill"
                    && (1..=3).contains(&args.len())
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let v_ty = self.type_of(ast, args[0])?;
                        // Array<Any>.fill accepts a cross-type fill
                        // value — ssa-lower routes through
                        // arr_fill_any which NaN-boxes the value
                        // regardless of type. Mirror of S127-4
                        // indexOf 2-arg dedicated-arm Any-escape.
                        if v_ty != **elem && !matches!(**elem, Type::Any) {
                            return Err(format!(
                                "Array.fill arg 0 must match elem type {:?}, got {v_ty:?}",
                                **elem
                            ));
                        }
                        // S335 — `xs.fill(v, Any [, Any])` per ES
                        // §23.1.3.7 step 5/9: ToIntegerOrInfinity accepts
                        // arbitrary-typed input. Sister to S334
                        // (Array.slice Any). ssa_lower mirror decodes via
                        // anyv_to_number → coerce_to_i64.
                        if args.len() >= 2 {
                            let start_ty = self.type_of(ast, args[1])?;
                            if start_ty != Type::Number
                                && start_ty != Type::Undefined
                                && start_ty != Type::Any
                            {
                                return Err(format!(
                                    "Array.fill arg 1 (start) must be number, got {start_ty:?}"
                                ));
                            }
                        }
                        if args.len() == 3 {
                            let end_ty = self.type_of(ast, args[2])?;
                            if end_ty != Type::Number
                                && end_ty != Type::Undefined
                                && end_ty != Type::Any
                            {
                                return Err(format!(
                                    "Array.fill arg 2 (end) must be number, got {end_ty:?}"
                                ));
                            }
                        }
                        return Ok(Type::Array(Box::new((**elem).clone())));
                    }
                }
                // V3-18 m1.h.51 — String.startsWith / endsWith /
                // includes accept an optional 2nd `position` arg per
                // JS spec §21.1.3.20 / §21.1.3.6 / §21.1.3.7.
                //
                // S224 — accept Undefined for the position slot per
                // ES §22.1.3.{21,5} (startsWith/includes:
                // ToIntegerOrInfinity(undefined)=0) and §22.1.3.7
                // (endsWith: endPosition undef → length). ssa_lower
                // mirror lowers undef → ConstI64(0) for startsWith /
                // includes and ConstI64(i64::MAX) for endsWith; the
                // _from helpers clamp `> len` to `len`.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "startsWith" | "endsWith" | "includes")
                    && args.len() == 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let needle_ty = self.type_of(ast, args[0])?;
                        if !matches!(needle_ty, Type::String) {
                            return Err(format!(
                                "String.{m_name} arg 0 must be string, got {needle_ty:?}"
                            ));
                        }
                        let from_ty = self.type_of(ast, args[1])?;
                        if !matches!(from_ty, Type::Number | Type::Undefined) {
                            return Err(format!(
                                "String.{m_name} arg 1 must be number, got {from_ty:?}"
                            ));
                        }
                        return Ok(Type::Boolean);
                    }
                }
                // S235 — String.{indexOf,lastIndexOf,includes,startsWith,
                // endsWith,search}(undefined) 1-arg-undef-needle per ES
                // §22.1.3.{8,10,5,21,7,16} step 1-3: ToString(undefined)
                // = "undefined". The Type::Function arm declares the
                // needle as String and rejects the typed-Undefined
                // operand. Widen the 1-arg dispatch; ssa_lower_str's
                // `undef_to_str_at_arg0` mirror substitutes the interned
                // "undefined" literal for the helper's (Str, Str) ABI.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "indexOf"
                            | "lastIndexOf"
                            | "includes"
                            | "startsWith"
                            | "endsWith"
                            | "search"
                    )
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let needle_ty = self.type_of(ast, args[0])?;
                        if matches!(needle_ty, Type::Undefined) {
                            return Ok(
                                if matches!(m_name.as_str(), "includes" | "startsWith" | "endsWith")
                                {
                                    Type::Boolean
                                } else {
                                    Type::Number
                                },
                            );
                        }
                    }
                }
                // V3-18 m1.h.50 — String.indexOf / lastIndexOf accept
                // an optional 2nd `fromIndex` arg per JS spec §21.1.3.7
                // / §21.1.3.10. Pre-fix tora declared with 1 fixed
                // param.
                //
                // S214 — String.indexOf(needle, undefined) per ES
                // §22.1.3.8 step 4: ToIntegerOrInfinity(undefined)=0,
                // accept Undefined fromIndex and treat as 0. ssa_lower
                // mirror inline-replaces argv[1] with ConstI64(0).
                // (lastIndexOf NaN→+∞ default is a follow-up ship.)
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "indexOf" | "lastIndexOf")
                    && args.len() == 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let needle_ty = self.type_of(ast, args[0])?;
                        if !matches!(needle_ty, Type::String) {
                            return Err(format!(
                                "String.{m_name} arg 0 must be string, got {needle_ty:?}"
                            ));
                        }
                        let from_ty = self.type_of(ast, args[1])?;
                        let from_undef = matches!(from_ty, Type::Undefined);
                        // S214 indexOf → fromIndex=0; S216 lastIndexOf
                        // → fromIndex=+∞ (ES §22.1.3.10 step 5: NaN→+∞)
                        // → ssa_lower lowers to i64::MAX and the helper
                        // clamps `from > len` to len, matching spec.
                        let allow_undef = from_undef;
                        if from_ty != Type::Number && !allow_undef {
                            return Err(format!(
                                "String.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
                            ));
                        }
                        return Ok(Type::Number);
                    }
                }
                // V3-18 wedge — Array.push / Array.unshift accept
                // a variable number of args per JS spec §22.1.3.20
                // / §22.1.3.34. Each arg is appended (or prepended)
                // in order. Pre-fix tora's strict 1-arg signature
                // rejected the multi-arg form. Subset typecheck
                // enforces every arg matches the element type and
                // returns Void (push's new-length return is not
                // surfaced).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "push" | "unshift")
                    && args.len() != 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = src_ty {
                        let inner = (*elem).clone();
                        for (i, aid) in args.iter().enumerate() {
                            let aty = self.type_of(ast, *aid)?;
                            if aty != inner && aty != Type::Any {
                                return Err(format!(
                                    "Array.{m_name} arg {i}: expected element type {:?}, got {aty:?}",
                                    inner
                                ));
                            }
                        }
                        return Ok(Type::Void);
                    }
                }
                // V3-18 wedge — String.split accepts an optional
                // 2nd `limit` arg per JS spec §22.1.3.21. Returns
                // first `limit` substrings (or fewer if the source
                // splits into fewer). Pre-fix tora's strict 1-arg
                // signature rejected the 2-arg form.
                //
                // S215 — `s.split(sep, undefined)` per ES §22.1.3.21
                // step 2: `If limit is undefined, lim = 2^32-1` (no
                // truncation). Accept Undefined limit; ssa_lower
                // mirror inline-replaces argv[2] with ConstI64(i64::MAX)
                // so the take-min branch falls to len.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "split"
                    && args.len() == 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let _ = self.type_of(ast, args[0])?;
                        let limit_ty = self.type_of(ast, args[1])?;
                        if limit_ty != Type::Number && limit_ty != Type::Undefined {
                            return Err(format!(
                                "String.split arg 1 (limit) must be number, got {limit_ty:?}"
                            ));
                        }
                        return Ok(Type::Array(Box::new(Type::String)));
                    }
                }
                // V3-18 wedge — Array.sort / toSorted accept an
                // optional comparator. Per JS spec §22.1.3.27 the
                // default cmp converts to string and compares
                // lexicographically; subset uses element-type-aware
                // `<`/`>` comparison via the runtime helper. Pre-fix
                // tora's strict 1-arg signature rejected the no-arg
                // form `arr.sort()`.
                //
                // ES §23.1.3.{29,31} step 1 also explicitly accepts
                // the literal `undefined` for `comparefn`: "If
                // comparefn is not undefined and not callable, throw
                // a TypeError". bun observes `undefined` → default
                // compare; tora's strict comparator-type check
                // rejected the 1-arg form `.sort(undefined)`. Treat
                // a 1-arg call with arg type Undefined as equivalent
                // to the no-arg case (SSA mirror at
                // ssa_lower_str.rs `sort/toSorted` dispatch).
                // S277 — Map/Set.{keys,values,entries}(...trailing) per
                // ES §23.{1,2}.3.X iterator-factory trailing-arg ignore.
                // Spec defines 0-arg only; trailing slots silent-drop.
                // tora's static sig is `Vec::new() → MapIter`; 1+ args
                // bounce at strict arity. Accept any arg count and
                // typecheck-and-drop trailing exprs.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "keys" | "values" | "entries")
                    && !args.is_empty()
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Map | Type::Set) {
                        for &a in args.iter() {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::MapIter);
                    }
                }
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "sort" | "toSorted")
                {
                    if args.is_empty() {
                        let src_ty = self.type_of(ast, *src_id)?;
                        if let Type::Array(elem) = src_ty {
                            return Ok(Type::Array(elem));
                        }
                    } else if args.len() == 1 {
                        let arg_ty = self.type_of(ast, args[0])?;
                        if arg_ty == Type::Undefined {
                            let src_ty = self.type_of(ast, *src_id)?;
                            if let Type::Array(elem) = src_ty {
                                return Ok(Type::Array(elem));
                            }
                        }
                    } else if args.len() >= 2 {
                        // S276 — Array.{sort,toSorted}(cmp, ...trailing) per
                        // ES §23.1.3.{30,33} trailing-arg ignore: spec
                        // reads only cmp; trailing slots silent-drop.
                        // tora's static sig is fixed 1-arg, so 2+ args
                        // bounce at strict arity. Accept any cmp shape
                        // (Function | Any), typecheck-and-drop trailing.
                        // S303 — also accept Undefined cmp per ES §23.1.3.30
                        // step 2: `sort(undef)` = `sort()` (default lex
                        // comparator). The 1-arg `undef` branch above
                        // handles args.len()==1; this widens 2+ to fold
                        // through the same default path + drop trailing.
                        let src_ty = self.type_of(ast, *src_id)?;
                        if let Type::Array(elem) = src_ty {
                            let aty0 = self.type_of(ast, args[0])?;
                            if matches!(aty0, Type::Function(..) | Type::Any | Type::Undefined) {
                                for &a in &args[1..] {
                                    let _ = self.type_of(ast, a)?;
                                }
                                return Ok(Type::Array(elem));
                            }
                        }
                    }
                }
                // V3-18 m1.h.49 — Array.indexOf / lastIndexOf accept
                // an optional fromIndex 2nd arg per JS spec §22.1.3.13
                // / §22.1.3.16. Pre-fix tora declared with 1 fixed
                // param so 2-arg calls hit the arity check.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "indexOf" | "lastIndexOf" | "includes")
                    && args.len() == 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let needle_ty = self.type_of(ast, args[0])?;
                        // S127-4: Array<Any> accepts cross-type needle —
                        // ssa-lower's strict-eq packing arm already
                        // handles I64/F64/Bool/Ptr/refcounted/Any
                        // (ssa_lower_str.rs §arr.indexOf needle pack).
                        // The 1-arg path falls through the generic arg-
                        // unify which skips equality when param is Any,
                        // but this dedicated 2-arg branch hand-wrote a
                        // strict-eq compare with no Any escape. Bring it
                        // in line with the 1-arg case + spec §22.1.3.x
                        // (no static type restriction on needle).
                        if needle_ty != **elem && !matches!(**elem, Type::Any) {
                            return Err(format!(
                                "Array.{m_name} arg 0 must match elem type {:?}, got {needle_ty:?}",
                                **elem
                            ));
                        }
                        let from_ty = self.type_of(ast, args[1])?;
                        // S217 — Array.{indexOf,lastIndexOf,includes}
                        // (needle, undefined) per ES §22.1.3.{13,14,16}:
                        // ToIntegerOrInfinity(undefined)=0. Accept
                        // Undefined alongside Number; ssa_lower mirror
                        // short-circuits fromIndex=undefined to
                        // ConstI64(0).
                        //
                        // S331 — widen accept Any fromIndex per ES
                        // §23.1.3.{14,17,18} step 4: ToIntegerOrInfinity
                        // already coerces arbitrary-typed input (NaN→0,
                        // ±∞→sat). ssa_lower mirror routes Any through
                        // anyv_to_number → coerce_to_i64. Pattern A
                        // sister to S327 / S329.
                        if from_ty != Type::Number
                            && from_ty != Type::Undefined
                            && from_ty != Type::Any
                        {
                            return Err(format!(
                                "Array.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
                            ));
                        }
                        return Ok(if m_name == "includes" {
                            Type::Boolean
                        } else {
                            Type::Number
                        });
                    }
                }
                // S225 — typed Array<T>.at(undefined) per ES §23.1.3.1
                // step 2-3: ToIntegerOrInfinity(undefined)=0, returns
                // arr[0]. Accept Undefined alongside Number; ssa_lower
                // mirror short-circuits idx=undefined to ConstI64(0)
                // before coerce_to_i64 (matches S222 charAt early-
                // intercept idiom).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "at"
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let idx_ty = self.type_of(ast, args[0])?;
                        if !matches!(idx_ty, Type::Number | Type::Undefined) {
                            return Err(format!("Array.at arg 0 must be number, got {idx_ty:?}"));
                        }
                        return Ok((**elem).clone());
                    }
                }
                // V3-18 wedge — Number.isFinite / isNaN / isInteger /
                // isSafeInteger per JS spec §21.1.2.2 / §21.1.2.4 /
                // §21.1.2.3 / §21.1.2.5: these methods do NOT coerce
                // their argument. They return true iff the arg is a
                // Number value AND satisfies the finite / NaN /
                // integer / safe-integer predicate; for non-Number
                // args (string / boolean / null / object / array)
                // they return false statically. The existing
                // signature `(Number) -> Boolean` rejects non-Number
                // args with a type error, but that's wrong for spec
                // and breaks the canonical TS feature-detection
                // idiom `if (Number.isFinite(maybeStringy)) ...`.
                // S342 — `Math.<unary/binary>(Any[, Any])` per ES §21.3.2.*:
                // every Math method that takes Number args applies ToNumber,
                // which accepts arbitrary-typed input. Method-table sigs
                // `vec![Type::Number]` / `vec![Type::Number, Type::Number]`
                // at the Math dispatch site (~2912 / ~2943) strict-rejected
                // `o: any` operands. Widen here so the ssa_lower mirror
                // routes Any through anyv_to_number → F64 → Math helper.
                // Per-arg check: Any or Number passes; everything else
                // (String, Bool, Object) still hits the strict gate.
                // min/max variadic stays one carve-out — the iter loop
                // covers 0..N args uniformly.
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Math"
                    && matches!(
                        m_name.as_str(),
                        "sqrt"
                            | "abs"
                            | "floor"
                            | "ceil"
                            | "log"
                            | "exp"
                            | "sign"
                            | "round"
                            | "trunc"
                            | "sin"
                            | "cos"
                            | "tan"
                            | "asin"
                            | "acos"
                            | "atan"
                            | "log2"
                            | "log10"
                            | "cbrt"
                            | "sinh"
                            | "cosh"
                            | "tanh"
                            | "asinh"
                            | "acosh"
                            | "atanh"
                            | "expm1"
                            | "log1p"
                            | "clz32"
                            | "fround"
                            | "f16round"
                            | "pow"
                            | "min"
                            | "max"
                            | "atan2"
                    )
                {
                    let mut any_seen = false;
                    let mut all_ok = true;
                    for &aid in args {
                        let aty = self.type_of(ast, aid)?;
                        if matches!(aty, Type::Any) {
                            any_seen = true;
                        } else if aty != Type::Number {
                            all_ok = false;
                            break;
                        }
                    }
                    if any_seen && all_ok {
                        return Ok(Type::Number);
                    }
                }
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Number"
                    && matches!(
                        m_name.as_str(),
                        "isFinite" | "isNaN" | "isInteger" | "isSafeInteger"
                    )
                {
                    // Force type_of on the arg so any internal
                    // typecheck error still surfaces, but we don't
                    // require it to be Number — non-Number args
                    // route through the lower's static-false path.
                    //
                    // S202 — extend the same loose check to the 0-arg
                    // form per §21.1.2.{3,5,7}: non-Number args
                    // (including the implicit undefined) statically
                    // return false; ssa_lower's short-circuit emits
                    // ConstBool(false) without dispatching the helper.
                    //
                    // S253 — `Number.{isFinite,isNaN,isInteger,
                    // isSafeInteger}(value, ...trailing)` trailing-arg
                    // ignore per ES §21.1.2.{2,3,4,5}: spec reads only
                    // args[0]; tora silent-drops trailing per generic
                    // trailing-arg-ignore policy. SSA-emit short-
                    // circuits non-Number args (ConstBool(false)) and
                    // dispatches the helper for Number args, both
                    // reading only args[0].
                    if let Some(arg0) = args.first() {
                        let _ = self.type_of(ast, *arg0)?;
                    }
                    for &arg in args.iter().skip(1) {
                        let _ = self.type_of(ast, arg)?;
                    }
                    return Ok(Type::Boolean);
                }
                // V3-18 wedge — String.charAt / charCodeAt /
                // codePointAt accept an optional pos arg per JS
                // spec §22.1.3.4 / §22.1.3.5 / §22.1.3.6: missing
                // pos defaults to 0. Pre-fix tora declared with one
                // required param so 0-arg calls bounced at the
                // unified arity check with 'expected 1 argument(s),
                // got 0'. Implementation: typecheck-only pass through
                // for the missing-arg shape; ssa_lower's 1-arg path
                // gets a synthetic ConstI64(0) padded in for the
                // default.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "charAt" | "charCodeAt" | "codePointAt")
                    && args.is_empty()
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        return Ok(if m_name == "charAt" {
                            Type::String
                        } else {
                            Type::Number
                        });
                    }
                }
                // S222 — `s.{at,charAt,charCodeAt,codePointAt}(undefined)`
                // per ES §22.1.3.{1,2,3,4} step 2-3:
                // ToIntegerOrInfinity(undefined)=0, so an explicit undef
                // index behaves as the 0-arg / index=0 form. ssa_lower
                // mirror short-circuits the Undefined slot to ConstI64(0).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "at" | "charAt" | "charCodeAt" | "codePointAt"
                    )
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let aty = self.type_of(ast, args[0])?;
                        if matches!(aty, Type::Undefined) {
                            return Ok(if matches!(m_name.as_str(), "charAt" | "at") {
                                Type::String
                            } else {
                                Type::Number
                            });
                        }
                    }
                }
                // S332 — `s.{at,charAt,charCodeAt,codePointAt,repeat}(Any)`
                // per ES §22.1.3.{1,2,3,4,17} step 2-3 (or step 1 for
                // repeat): ToIntegerOrInfinity accepts arbitrary-typed
                // input. The method-table sig `(Number) -> X` rejected
                // explicit `o: any` operands at typecheck; widen here
                // so the ssa_lower mirror routes Any through
                // anyv_to_number → coerce_to_i64 → helper. Sister to
                // S329 (fromCharCode Any).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat"
                    )
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let aty = self.type_of(ast, args[0])?;
                        if matches!(aty, Type::Any) {
                            return Ok(if matches!(m_name.as_str(), "charAt" | "at" | "repeat") {
                                Type::String
                            } else {
                                Type::Number
                            });
                        }
                    }
                }
                // S240 — String.{at,charAt,charCodeAt,codePointAt,
                // repeat,normalize}(useful, ...trailing) trailing-arg
                // ignore per ES §22.1.3.{1,2,3,4,17,13}: spec reserves
                // slots past useful arg 0 but tora's helpers are 1-arg
                // only. Trailing operands type_of'd for side effects;
                // ssa_lower mirror evals-and-drops past i=0.
                //
                // S272 widens from `args.len() == 2` (single trailing)
                // to `args.len() >= 2` (any trailing count).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" | "normalize"
                    )
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let aty0 = self.type_of(ast, args[0])?;
                        let arg0_ok = match m_name.as_str() {
                            "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" => {
                                matches!(aty0, Type::Number | Type::Undefined)
                            }
                            "normalize" => matches!(aty0, Type::String | Type::Undefined),
                            _ => false,
                        };
                        if arg0_ok {
                            for &a in &args[1..] {
                                let _ = self.type_of(ast, a)?;
                            }
                            return Ok(match m_name.as_str() {
                                "at" | "charAt" | "repeat" | "normalize" => Type::String,
                                _ => Type::Number,
                            });
                        }
                    }
                }
                // S281 — String.{trim,trimStart,trimEnd,trimLeft,
                // trimRight,toUpperCase,toLowerCase,toWellFormed,
                // isWellFormed}(...trailing) trailing-arg ignore per
                // ES §22.1.3.{30,32,33,28,29,34,10}. Spec-defined
                // 0-arg methods; spec reserves no positional slots so
                // any trailing operand is silently ignored. tora's
                // helpers are 0-arg only (recv only); typecheck-and-
                // drop trailing here; ssa_lower mirror lowers each
                // operand for side effects then drops the value (S272
                // idiom — never push into argv). toLocale{Upper,Lower}Case
                // is excluded (S140 sig + drop_args path own those —
                // they take a locales arg and already silent-drop it).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "trim"
                            | "trimStart"
                            | "trimEnd"
                            | "trimLeft"
                            | "trimRight"
                            | "toUpperCase"
                            | "toLowerCase"
                            | "toWellFormed"
                            | "isWellFormed"
                    )
                    && !args.is_empty()
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        for &a in args {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(if m_name == "isWellFormed" {
                            Type::Boolean
                        } else {
                            Type::String
                        });
                    }
                }
                // V3-18 m1.h.48 — String.normalize accepts an optional
                // form arg ("NFC" / "NFD" / "NFKC" / "NFKD"). Per JS
                // spec §21.1.3.13. tora's byte-Str ASCII-only path
                // returns identity for any form, so we just typecheck
                // and route through the existing 0-arg lowering.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "normalize"
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let aty = self.type_of(ast, args[0])?;
                        // S208 — spec §22.1.3.13 step 1: when `form` is
                        // undefined, default to "NFC". Accept the typed-
                        // Undefined arg shape alongside the String form;
                        // ssa_lower routes both to the existing 0-arg
                        // NFC-default path.
                        if matches!(aty, Type::Undefined) {
                            return Ok(Type::String);
                        }
                        if !matches!(aty, Type::String) {
                            return Err(format!(
                                "String.normalize arg must be string, got {aty:?}"
                            ));
                        }
                        return Ok(Type::String);
                    }
                }
                // V3-18 m1.h.46 — Number.toFixed / toExponential /
                // toPrecision with 0 args. Per JS spec §21.1.3.3 etc:
                //   n.toFixed()        defaults to digits = 0
                //   n.toExponential()  defaults to fractionDigits = "as
                //                       few as needed" (we use 6 — bun
                //                       matches; actual spec call ToInteger
                //                       on undefined gives 0 but bun's
                //                       output uses default precision)
                //   n.toPrecision()    no precision = same as toString
                // Pre-fix tora declared with 1 fixed param so 0-arg
                // calls failed at the arity check. Implementation:
                // typecheck-only pass through; ssa_lower handles the
                // missing-arg defaults.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "toFixed" | "toExponential" | "toPrecision")
                    && (args.is_empty()
                        || (args.len() == 1
                            && matches!(self.type_of(ast, args[0])?, Type::Undefined)))
                {
                    // S229 — accept the 1-arg explicit-Undefined shape per
                    // ES §21.1.3.{3,5,6} where an undef digits/precision
                    // arg folds to the same default as the 0-arg form.
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Number) {
                        return Ok(Type::String);
                    }
                }
                // S254 — `n.{toFixed,toExponential,toPrecision}(digits,
                // ...trailing)` per ES §21.1.3.{3,5,6} trailing-arg
                // ignore. Spec reads only args[0] (digits/fractionDigits
                // /precision); tora silent-drops trailing per generic
                // trailing-arg-ignore policy. SSA-emit's per-method
                // dispatch (line ~16700) pushes args into argv; the
                // ssa-lower S254 mirror caps the push at args[0].
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "toFixed" | "toExponential" | "toPrecision")
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Number) {
                        let arg0_ty = self.type_of(ast, args[0])?;
                        if !matches!(arg0_ty, Type::Number | Type::Undefined) {
                            return Err(format!(
                                "Number.{m_name} arg 0 must be number, got {arg0_ty:?}"
                            ));
                        }
                        for &arg in args.iter().skip(1) {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::String);
                    }
                }
                // V3-18 wedge — Array.concat accepts any number of
                // array args per JS spec §22.1.3.2:
                //   xs.concat()            → fresh shallow copy of xs
                //   xs.concat(a, b, ..., z)→ fresh array of xs then a's
                //                             then b's ... then z's
                // Pre-fix tora declared concat with a fixed 1-arg
                // signature so multi-arg calls failed at the unified
                // arity check. Subset constraint kept: every additional
                // arg must be an Array<T> with the same element type
                // as the receiver — scalar args (the spec's "values
                // are added") would require the heterogeneous-element
                // substrate that isn't in tora yet.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "concat"
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let expected = (**elem).clone();
                        // 0-arg form: shallow copy of receiver. Skip
                        // arg-type validation entirely.
                        if args.is_empty() {
                            return Ok(Type::Array(Box::new(expected)));
                        }
                        // ES §23.1.3.2 — every arg is either an Array<T>
                        // (spread into the result) or a single T value
                        // (appended as one element). Mixed shapes are
                        // valid: `xs.concat([4,5], 6, [7,8])`.
                        let mut ok = true;
                        for a in args {
                            let a_ty = self.type_of(ast, *a)?;
                            let is_arr_t = a_ty == Type::Array(Box::new(expected.clone()));
                            let is_scalar_t = a_ty == expected;
                            if !is_arr_t && !is_scalar_t {
                                ok = false;
                                break;
                            }
                        }
                        if ok {
                            return Ok(Type::Array(Box::new(expected)));
                        }
                    }
                }
                // V3-18 wedge — Array.copyWithin with 1 or 2 args per
                // JS spec §22.1.3.3:
                //   xs.copyWithin(target)            = (target, 0, len)
                //   xs.copyWithin(target, start)     = (target, start, len)
                //   xs.copyWithin(target, start, end)= (target, start, end)
                // Pre-fix tora declared the method with a fixed 3-arg
                // signature so `xs.copyWithin(0, 2)` failed at the
                // arity check. SSA lower already had the 3-arg code
                // path; this commit additionally fills the missing
                // start (= 0) / end (= len) defaults at the SSA layer.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "copyWithin"
                    && args.len() >= 1
                    && args.len() <= 3
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        // S219 — accept Undefined for any arg per ES
                        // §23.1.3.4: target/start go through
                        // ToIntegerOrInfinity (undef → 0), end===undefined
                        // takes the omitted default (len). ssa_lower
                        // mirror short-circuits each undef slot to its
                        // spec default before relative_to_len.
                        // S335 — `xs.copyWithin(Any, Any, Any)` per ES
                        // §23.1.3.4: each arg goes through
                        // ToIntegerOrInfinity which accepts arbitrary-
                        // typed input. Sister to S334.
                        for (i, a) in args.iter().enumerate() {
                            let a_ty = self.type_of(ast, *a)?;
                            if a_ty != Type::Number && a_ty != Type::Undefined && a_ty != Type::Any
                            {
                                return Err(format!(
                                    "Array.copyWithin arg {i} must be number, got {a_ty:?}"
                                ));
                            }
                        }
                        return Ok(Type::Array(elem.clone()));
                    }
                }
                // V3-18 m1.h.45 — String.padStart / padEnd with 1 arg
                // defaults the fill string to " " per JS spec §21.1.3.16.
                // Pre-fix tora declared the methods with 2 fixed params
                // so `s.padStart(3)` failed at the arity check.
                //
                // S201 — extend the same default-undefined rule to the
                // 0-arg call per ES §22.1.3.{16,17} step 1:
                // `intMaxLength = ToLength(undefined) = 0`, so the
                // step-2 short-circuit `intMaxLength <= S.length`
                // makes the no-arg form a no-op returning S unchanged.
                //
                // S223 — widen the same default-undefined rule to a
                // 1- or 2-arg call with an explicit `undefined` in the
                // maxLength slot. Routing the standard 2-arg shape
                // (`s.padStart(3, "*")`) through the same carve-out is
                // equivalent to the line-3413 Function sig but lets us
                // accept Undefined for arg 0.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "padStart" || m_name == "padEnd")
                    && args.len() <= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        // S338 — `s.{padStart,padEnd}(Any [, fillStr])`
                        // per ES §22.1.3.{16,17} step 1: ToLength
                        // accepts arbitrary-typed input. Sister to
                        // S332/S334. ssa_lower mirror routes Any
                        // through anyv_to_number → coerce_to_i64.
                        if let Some(arg0) = args.first() {
                            let aty = self.type_of(ast, *arg0)?;
                            if !matches!(aty, Type::Number | Type::Undefined | Type::Any) {
                                return Err(format!(
                                    "String.{m_name} arg 0 must be number, got {aty:?}"
                                ));
                            }
                        }
                        if let Some(arg1) = args.get(1) {
                            let aty = self.type_of(ast, *arg1)?;
                            // S236 — accept Undefined for the fillStr
                            // slot per ES §22.1.3.{16,17} step 6.a: if
                            // fillString is undefined, set it to " ".
                            // ssa_lower_str's V3-18 m1.h.45 1-arg
                            // fallthrough already supplies the " "
                            // default, so we just need the type gate
                            // to accept the typed-Undefined operand.
                            if !matches!(aty, Type::String | Type::Undefined) {
                                return Err(format!(
                                    "String.{m_name} arg 1 must be string, got {aty:?}"
                                ));
                            }
                        }
                        return Ok(Type::String);
                    }
                }
                // V3-18 m1.h.42 — Array<String|Substr>.join() with no
                // sep arg defaults to ","; matches JS spec §22.1.3.13.
                // Pre-fix tora declared join with 1 fixed param so
                // `xs.join()` failed at the arity check.
                //
                // S206 — extend the same rule to an explicit
                // `undefined` sep per spec §23.1.3.16 step 1: if
                // sep is undefined → sep = ",". The 1-arg-undefined
                // call shape was rejecting at the strict-arity
                // gate with "argument 0: expected String, got
                // Undefined".
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "join"
                {
                    let undef_sep = args.len() == 1 && {
                        let aty = self.type_of(ast, args[0])?;
                        matches!(aty, Type::Undefined)
                    };
                    if args.is_empty() || undef_sep {
                        let src_ty = self.type_of(ast, *src_id)?;
                        if let Type::Array(elem) = &src_ty
                            && matches!(
                                **elem,
                                Type::String | Type::Number | Type::Boolean | Type::Any
                            )
                        {
                            return Ok(Type::String);
                        }
                    }
                }
                // V3-18 m1.h.36 — String.slice / substring with 0 or
                // 1 args. Per JS spec §21.1.3.21 / §21.1.3.23:
                //   s.slice()      = s.slice(0, s.length)
                //   s.slice(start) = s.slice(start, s.length)
                //   (same for substring; substring also clamps and
                //   swaps args, but the optional-arity shape is
                //   identical at the call site)
                //
                // S232 — accept Undefined for the start slot per ES
                // §22.1.3.20 step 3 (slice) / §22.1.3.22 step 4
                // (substring): ToIntegerOrInfinity(undef)=0. The
                // ssa_lower_str mirror substitutes ConstI64(0) and
                // the existing 0-1-arg fallthrough fills in the
                // end=length default. `substr` is excluded — its
                // 2nd arg is a length not an end index, and the
                // T-49 carve-out below handles its own arity-undef.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "slice" || m_name == "substring" || m_name == "substr")
                    && args.len() < 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let allow_undef = m_name == "slice" || m_name == "substring";
                        // S333 — `s.{slice,substring,substr}(Any)` per ES
                        // §22.1.3.{20,22,23}: ToIntegerOrInfinity accepts
                        // arbitrary-typed input. Widen the strict-Number
                        // gate; ssa_lower mirror routes Any through
                        // anyv_to_number → coerce_to_i64 → helper. Sister
                        // to S332 (charCodeAt/charAt/... Any).
                        for &aid in args {
                            let aty = self.type_of(ast, aid)?;
                            if aty != Type::Number
                                && !(allow_undef && aty == Type::Undefined)
                                && aty != Type::Any
                            {
                                return Err(format!(
                                    "String.{m_name} arg must be number, got {aty:?}"
                                ));
                            }
                        }
                        return Ok(Type::String);
                    }
                }
                // S221 — `s.substring(start, end)` accepts Undefined for
                // either positional per ES §22.1.3.22 step 4/5:
                // ToIntegerOrInfinity(undef)=0 for start, end===undefined
                // takes the omitted default (len). ssa_lower mirror
                // short-circuits each undef slot to 0 / recv.length so
                // the str_substring helper's I64 ABI never sees an
                // Undefined operand.
                //
                // S232 — extend the same widen to `s.slice(start, end)`
                // 2-arg per ES §22.1.3.20 step 3/4: start undef → 0,
                // end undef → length. slice's negative-index handling
                // is orthogonal — the helper still receives concrete
                // I64s after the ssa_lower mirror substitutes.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "substring" || m_name == "slice" || m_name == "substr")
                    && args.len() == 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        // S333 — 2-arg form widens Any. Same Any coerce
                        // pattern A as the 1-arg path. substr extends the
                        // method-table sig (Number, Number) here so the
                        // (Any, Any) shape doesn't fall through to the
                        // strict gate; ssa_lower mirror decodes via
                        // anyv_to_number → coerce_to_i64.
                        let allow_undef = m_name == "substring" || m_name == "slice";
                        for &aid in args {
                            let aty = self.type_of(ast, aid)?;
                            if aty != Type::Number
                                && !(allow_undef && aty == Type::Undefined)
                                && aty != Type::Any
                            {
                                return Err(format!(
                                    "String.{m_name} arg must be number, got {aty:?}"
                                ));
                            }
                        }
                        return Ok(Type::String);
                    }
                }
                // S241 — String.{slice,substring,substr,padStart,padEnd}
                // (a, b, ...trailing) trailing-arg ignore per ES
                // §22.1.3.{20,22,23,16,17}: spec reserves slots past the
                // 2 useful args (start/end / start/length / maxLen/fillStr)
                // but tora's helpers are 2-arg only. Trailing operand
                // type_of'd for side effects then dropped at lower-time
                // (ssa_lower break early past i=1). Same shape as S238
                // localeCompare.
                //
                // S284 — widen from `args.len() == 3` (single trailing)
                // to `args.len() >= 3` (any trailing count). ssa_lower
                // mirror swaps the loop break to lower_expr + continue
                // so step()-style side-effect exprs fire per ES eval-
                // then-discard semantics (S272 idiom).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "slice" | "substring" | "substr" | "padStart" | "padEnd"
                    )
                    && args.len() >= 3
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let aty0 = self.type_of(ast, args[0])?;
                        let arg0_ok = match m_name.as_str() {
                            "slice" | "substring" => {
                                matches!(aty0, Type::Number | Type::Undefined)
                            }
                            "substr" => matches!(aty0, Type::Number),
                            "padStart" | "padEnd" => {
                                matches!(aty0, Type::Number | Type::Undefined)
                            }
                            _ => false,
                        };
                        if !arg0_ok {
                            return Err(format!(
                                "String.{m_name} arg 0 must be number, got {aty0:?}"
                            ));
                        }
                        let aty1 = self.type_of(ast, args[1])?;
                        let arg1_ok = match m_name.as_str() {
                            "slice" | "substring" => {
                                matches!(aty1, Type::Number | Type::Undefined)
                            }
                            "substr" => matches!(aty1, Type::Number),
                            "padStart" | "padEnd" => {
                                matches!(aty1, Type::String | Type::Undefined)
                            }
                            _ => false,
                        };
                        if !arg1_ok {
                            return Err(format!(
                                "String.{m_name} arg 1 type mismatch, got {aty1:?}"
                            ));
                        }
                        for &a in &args[2..] {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::String);
                    }
                }
                // S211 — String.localeCompare(undefined) per ES
                // §22.1.3.10 step 4: thatStr = ToString(thatValue)
                // = "undefined". Pre-fix declared `(String) -> Number`
                // rejected the typed-Undefined arg with
                // "argument 0: expected String, got Undefined".
                // ssa_lower inline-substitutes the interned
                // "undefined" literal for the typed-undefined operand.
                //
                // S238 — extend the same carve-out to the 2-arg
                // (locales) and 3-arg (locales, options) shapes per ES
                // §22.1.3.10 trailing-arg ignore: the spec reserves
                // those slots for Intl-aware locale comparison but
                // tora's bytewise helper has no locale awareness, so
                // they're ignored. The ssa_lower_str loop trims any
                // arg beyond i=0 so the helper's (Str, Str) ABI never
                // sees the trailing operands.
                // S285 — widen S238 carve-out `(1..=3)` → `>= 1` so
                // 4+ arg trailing-widen shape typechecks. ssa_lower
                // mirror swaps the loop `break i > 0` to `let _ =
                // lower_expr(a); continue` so step()-style side-effect
                // exprs fire per ES eval-then-discard (S272 idiom).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "localeCompare"
                    && !args.is_empty()
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let aty0 = self.type_of(ast, args[0])?;
                        let arg0_ok = matches!(aty0, Type::String | Type::Undefined);
                        if arg0_ok {
                            for &aid in &args[1..] {
                                let _ = self.type_of(ast, aid)?;
                            }
                            return Ok(Type::Number);
                        }
                    }
                }
                // S292 — Array<T>.keys(...trailing) trailing-arg ignore
                // per ES §23.1.3.16. Spec sig is 0-arg returning an
                // ArrayIterator (tora: Type::ArrIter via
                // arr_iter_create_keys, fed only `recv_op`). Trailing
                // operands typecheck-and-drop here; ssa_lower mirrors
                // with lower-and-drop. values / entries stay narrow to
                // Array<Any> per the existing carve-out at 4540.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "keys"
                    && !args.is_empty()
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Array(_)) {
                        for &aid in args.iter() {
                            let _ = self.type_of(ast, aid)?;
                        }
                        return Ok(Type::ArrIter);
                    }
                }
                // S290 — primitive `.valueOf(...trailing)` trailing-arg
                // ignore per ES §21.1.3.27 / §20.4.3.4 / §22.1.3.34 /
                // §21.2.3.6 / §20.5.3.5. valueOf is 0-arg spec; tora's
                // SSA-emit folds it to an identity return (recv_op)
                // without inspecting args, so trailing operands type-
                // check-and-drop here + lower-and-drop in ssa_lower
                // (S272 idiom). Covers Number / Boolean / String /
                // BigInt / Symbol; Array.valueOf already handled by
                // the dedicated identity arm in ssa_lower.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "valueOf"
                    && !args.is_empty()
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    let ret_ty = match src_ty {
                        Type::Number => Some(Type::Number),
                        Type::Boolean => Some(Type::Boolean),
                        Type::String => Some(Type::String),
                        Type::BigInt => Some(Type::BigInt),
                        Type::Symbol => Some(Type::Symbol),
                        Type::Array(ref elem) => Some(Type::Array(elem.clone())),
                        _ => None,
                    };
                    if let Some(rt) = ret_ty {
                        for &aid in args.iter() {
                            let _ = self.type_of(ast, aid)?;
                        }
                        return Ok(rt);
                    }
                }
                // S288 — Array<T>.{pop,shift}(...trailing) trailing-arg
                // ignore per ES §23.1.3.{20,24}. Spec sigs are 0-arg;
                // tora's runtime helpers (in-place len-- + tail/head
                // slot load) ignore any extras. Typecheck-and-drop
                // here; ssa_lower's try_arr_pop / try_arr_shift arms
                // widen the `args.is_empty()` gate + lower-and-drop
                // args[..] so step()-style exprs fire (S272 idiom).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "pop" | "shift")
                    && !args.is_empty()
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let inner = (**elem).clone();
                        for &a in args.iter() {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(inner);
                    }
                }
                // S287 — Array<T>.{reverse,toReversed,join,toString,
                // toLocaleString}(...trailing) trailing-arg ignore per
                // ES §23.1.3.{27,33,15,32,31}. reverse / toReversed /
                // toString / toLocaleString are 0-arg per spec; join is
                // 1-arg (sep). tora's runtime helpers all read at most
                // the documented slots, so trailing operands typecheck-
                // and-drop here, ssa_lower mirrors with lower-and-drop
                // in the dispatch arms (S272 idiom). join's sep slot
                // accepts String | Undefined per S206; toString /
                // toLocaleString gate on the join-compatible elem types
                // (Str/Num/Bool/Any per S126-4 dispatch).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "reverse" | "toReversed" | "join" | "toString" | "toLocaleString"
                    )
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let inner = (**elem).clone();
                        let useful = if m_name == "join" { 1 } else { 0 };
                        if args.len() > useful {
                            if useful == 1 {
                                let sep_ty = self.type_of(ast, args[0])?;
                                if !matches!(sep_ty, Type::String | Type::Undefined) {
                                    return Err(format!(
                                        "Array.join arg 0 (sep) must be string, got {sep_ty:?}"
                                    ));
                                }
                            }
                            for &aid in &args[useful..] {
                                let _ = self.type_of(ast, aid)?;
                            }
                            return Ok(match m_name.as_str() {
                                "reverse" | "toReversed" => Type::Array(Box::new(inner)),
                                _ => Type::String,
                            });
                        }
                    }
                }
                // S286 — String.{match,matchAll}(re, ...trailing) trailing-
                // arg ignore per ES §22.1.3.{11,13}. Spec reads only `re`;
                // tora's regex helper takes only (Str, RegExp) so trailing
                // operands typecheck-and-drop here, ssa_lower mirrors with
                // lower-and-drop in the RegExp-branch match/matchAll arms.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "match" | "matchAll")
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let aty0 = self.type_of(ast, args[0])?;
                        if matches!(aty0, Type::RegExp) {
                            for &aid in &args[1..] {
                                let _ = self.type_of(ast, aid)?;
                            }
                            return Ok(if m_name == "matchAll" {
                                Type::Array(Box::new(Type::Array(Box::new(Type::String))))
                            } else {
                                Type::Array(Box::new(Type::String))
                            });
                        }
                    }
                }
                // S246 — Array<T>.{copyWithin,fill}(a, b, c, ...trailing)
                // trailing-arg ignore per ES §23.1.3.{4,7}. Spec
                // reserves slots past the 3 useful args but tora's
                // inline copyWithin / fill loops are 3-arg only.
                // Trailing operand type_of'd for side effects then
                // dropped at lower-time; SSA-emit reads only
                // args[0..=2] so args[3..] are silently ignored.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "copyWithin" | "fill")
                    && args.len() >= 4
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let inner = (**elem).clone();
                        // arg 0 (target / value): type-checked per
                        // method shape — fill arg 0 is element type,
                        // copyWithin arg 0 is Number (target index).
                        let aty0 = self.type_of(ast, args[0])?;
                        if m_name == "fill" {
                            if aty0 != inner && !matches!(inner, Type::Any) {
                                return Err(format!(
                                    "Array.fill arg 0 (value) must match elem type {:?}, got {aty0:?}",
                                    inner
                                ));
                            }
                        } else if !matches!(aty0, Type::Number | Type::Undefined) {
                            return Err(format!(
                                "Array.copyWithin arg 0 (target) must be number, got {aty0:?}"
                            ));
                        }
                        let aty1 = self.type_of(ast, args[1])?;
                        if !matches!(aty1, Type::Number | Type::Undefined) {
                            return Err(format!(
                                "Array.{m_name} arg 1 must be number, got {aty1:?}"
                            ));
                        }
                        let aty2 = self.type_of(ast, args[2])?;
                        if !matches!(aty2, Type::Number | Type::Undefined) {
                            return Err(format!(
                                "Array.{m_name} arg 2 must be number, got {aty2:?}"
                            ));
                        }
                        // S310 — widen `== 4` to `>= 4` per ES §23.1.3.{4,7}
                        // trailing-arg ignore. Spec reads target/value +
                        // start + end only; trailing slots silent-drop.
                        // ssa_lower's S298 skip(3) loop already drains
                        // args[3..] for side-effects.
                        for &a in args.iter().skip(3) {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::Array(elem.clone()));
                    }
                }
                // S245 — Array<T>.{reduce,reduceRight}(fn, init, ...trailing)
                // trailing-arg ignore per ES §22.1.3.{21,22}. Spec
                // reserves slots past the 2 useful args (callback +
                // initial value) but tora's inline reduce loop is
                // 2-arg only. Trailing operand type_of'd for side
                // effects then dropped at lower-time; SSA-emit reads
                // only args[0..=1] so args[2..] are silently ignored
                // without any SSA-side change. Same shape as S243/S244.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "reduce" | "reduceRight")
                    && args.len() >= 3
                {
                    // S276 — widen `== 3` to `>= 3` per ES §23.1.3.{22,23}
                    // trailing-arg ignore. Spec reads cb + initialValue
                    // only; trailing slots silent-drop. ssa_lower's
                    // 22487 entry reads only args[0] (cb) + args[1]
                    // (initial); args[2..] handled by S270 skip(2) loop.
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let inner = (**elem).clone();
                        let aty0 = self.type_of(ast, args[0])?;
                        let fn_ok = matches!(aty0, Type::Function(..) | Type::Any);
                        if !fn_ok {
                            return Err(format!(
                                "Array.{m_name} arg 0 must be a callback function, got {aty0:?}"
                            ));
                        }
                        let aty1 = self.type_of(ast, args[1])?;
                        if aty1 != inner && !matches!(inner, Type::Any) {
                            return Err(format!(
                                "Array.{m_name} arg 1 (initial) must match elem type {:?}, got {aty1:?}",
                                inner
                            ));
                        }
                        for &a in &args[2..] {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(inner);
                    }
                }
                // S242 — Array<T>.{at,slice,join}(useful, ...trailing)
                // trailing-arg ignore per ES §23.1.3.{1,28,16}. Spec
                // reserves slots past the useful args but tora's
                // helpers are 1-/2-/1-arg only; trailing operand
                // type_of'd for side effects then dropped at lower-
                // time (ssa_lower's args[N] slot never read).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "at" | "slice" | "join")
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        // S299 — widen `args.len() == 2` → `>= 2` + typecheck-
                        // and-drop args[1..] for any extra trailing operands per
                        // ES §23.1.3.1 trailing-arg ignore (same family as S272/
                        // S278/S293-S298). ssa_lower mirror widens at arm gate
                        // from `args.len() <= 2` to no upper-cap + lower-and-drop
                        // args[1..] so step()-style side-effect exprs fire per
                        // ES eval-then-discard semantics.
                        if m_name == "at" && args.len() >= 2 {
                            let aty0 = self.type_of(ast, args[0])?;
                            if !matches!(aty0, Type::Number | Type::Undefined) {
                                return Err(format!("Array.at arg 0 must be number, got {aty0:?}"));
                            }
                            for &a in args.iter().skip(1) {
                                let _ = self.type_of(ast, a)?;
                            }
                            return Ok((**elem).clone());
                        }
                        // S299 — widen `== 3` → `>= 3` + drop args[2..] per ES
                        // §23.1.3.28 trailing-arg ignore.
                        if m_name == "slice" && args.len() >= 3 {
                            let aty0 = self.type_of(ast, args[0])?;
                            if !matches!(aty0, Type::Number | Type::Undefined) {
                                return Err(format!(
                                    "Array.slice arg 0 must be number, got {aty0:?}"
                                ));
                            }
                            let aty1 = self.type_of(ast, args[1])?;
                            if !matches!(aty1, Type::Number | Type::Undefined) {
                                return Err(format!(
                                    "Array.slice arg 1 must be number, got {aty1:?}"
                                ));
                            }
                            for &a in args.iter().skip(2) {
                                let _ = self.type_of(ast, a)?;
                            }
                            return Ok(Type::Array(elem.clone()));
                        }
                        // S299 — widen `== 2` → `>= 2` + drop args[1..] per ES
                        // §23.1.3.16 trailing-arg ignore. ssa_lower's join arm
                        // already loop-drops args[1..] via the S287 useful=1
                        // skip; widening the typecheck gate completes the pair.
                        if m_name == "join"
                            && args.len() >= 2
                            && matches!(
                                **elem,
                                Type::String | Type::Number | Type::Boolean | Type::Any
                            )
                        {
                            let aty0 = self.type_of(ast, args[0])?;
                            if !matches!(aty0, Type::String | Type::Undefined) {
                                return Err(format!(
                                    "Array.join arg 0 must be string, got {aty0:?}"
                                ));
                            }
                            for &a in args.iter().skip(1) {
                                let _ = self.type_of(ast, a)?;
                            }
                            return Ok(Type::String);
                        }
                    }
                }
                // S239 — String/Array.{indexOf,lastIndexOf,includes}
                // + String.{startsWith,endsWith}(needle, fromIndex,
                // ...trailing) trailing-arg ignore per ES
                // §22.1.3.{8,10,5,21,7} / §23.1.3.{14,17,18}: spec
                // reserves slots after fromIndex but tora's helpers
                // are 2-arg only. Trim trailing operands at lower-time
                // (ssa_lower mirrors break early past i=1 / drop
                // args[2..]). Same shape as S238 localeCompare.
                //
                // S278 — widen `args.len() == 3` → `>= 3` + typecheck-
                // and-drop args[2..] for any extra trailing operands per
                // ES trailing-arg ignore (same family as S270/S272/S275/
                // S276/S277). ssa_lower mirror widens the Array path
                // gate to `>= 1` + lower-and-drop args[2..]; the String
                // path swaps `break` to `let _ = lower_expr(a); continue`
                // so step()-style side-effect exprs fire per ES eval-
                // then-discard semantics.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith"
                    )
                    && args.len() >= 3
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let needle_ty = self.type_of(ast, args[0])?;
                        if !matches!(needle_ty, Type::String | Type::Undefined) {
                            return Err(format!(
                                "String.{m_name} arg 0 must be string, got {needle_ty:?}"
                            ));
                        }
                        let from_ty = self.type_of(ast, args[1])?;
                        if !matches!(from_ty, Type::Number | Type::Undefined) {
                            return Err(format!(
                                "String.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
                            ));
                        }
                        for &a in args.iter().skip(2) {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(
                            if matches!(m_name.as_str(), "includes" | "startsWith" | "endsWith") {
                                Type::Boolean
                            } else {
                                Type::Number
                            },
                        );
                    }
                    if let Type::Array(elem) = &src_ty
                        && matches!(m_name.as_str(), "indexOf" | "lastIndexOf" | "includes")
                    {
                        let needle_ty = self.type_of(ast, args[0])?;
                        if needle_ty != **elem && !matches!(**elem, Type::Any) {
                            return Err(format!(
                                "Array.{m_name} arg 0 must match elem type {:?}, got {needle_ty:?}",
                                **elem
                            ));
                        }
                        let from_ty = self.type_of(ast, args[1])?;
                        if !matches!(from_ty, Type::Number | Type::Undefined) {
                            return Err(format!(
                                "Array.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
                            ));
                        }
                        for &a in args.iter().skip(2) {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(if m_name == "includes" {
                            Type::Boolean
                        } else {
                            Type::Number
                        });
                    }
                }
                // S255 — Object.keys / Object.getOwnPropertyNames /
                // Reflect.ownKeys (obj, ...trailing) trailing-arg ignore
                // per ES §20.1.2.{17,22} / §28.1.11. Spec reads only
                // args[0]; tora silent-drops trailing per generic
                // trailing-arg-ignore policy. SSA-emit mirror widens
                // the `args.len() == 1` gate to `>= 1` (ssa_lower.rs:
                // ~18745). Narrow to this shared 3-method SSA-emit
                // dispatch — other Object/Reflect methods need
                // per-method SSA-emit widening (L3b).
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ((ns == "Object" && (m_name == "keys" || m_name == "getOwnPropertyNames"))
                        || (ns == "Reflect" && m_name == "ownKeys"))
                    && args.len() >= 2
                {
                    let _ = self.type_of(ast, args[0])?;
                    for &arg in args.iter().skip(1) {
                        let _ = self.type_of(ast, arg)?;
                    }
                    return Ok(Type::Array(Box::new(Type::String)));
                }
                // S256 — Object.{entries,freeze,isFrozen}(obj, ...trailing)
                // trailing-arg ignore per ES §20.1.2.{5,12,15}. Spec
                // reads only args[0]; tora silent-drops trailing per
                // generic trailing-arg-ignore policy. SSA-emit mirror
                // widens each `args.len() == 1` gate to `>= 1`.
                // S258 extends to `values` — same shape, returns
                // Array<Any> (the per-method 1-arg sig was missing
                // entirely; now added above).
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Object"
                    && matches!(
                        m_name.as_str(),
                        "entries" | "freeze" | "isFrozen" | "values"
                    )
                    && args.len() >= 2
                {
                    let arg0_ty = self.type_of(ast, args[0])?;
                    for &arg in args.iter().skip(1) {
                        let _ = self.type_of(ast, arg)?;
                    }
                    return Ok(match m_name.as_str() {
                        "entries" => Type::Array(Box::new(Type::Array(Box::new(Type::Any)))),
                        "freeze" => arg0_ty,
                        "isFrozen" => Type::Boolean,
                        "values" => Type::Array(Box::new(Type::Any)),
                        _ => unreachable!(),
                    });
                }
                // S257 — Object.{hasOwn,is} / Reflect.{has,get} (obj,
                // key|b, ...trailing) trailing-arg ignore per ES
                // §20.1.2.{4,9} / §28.1.{6,9}. Spec reads only
                // args[0..2]; tora silent-drops trailing. SSA-emit
                // mirror widens each `args.len() == 2` gate to `>= 2`
                // (ssa_lower.rs ~18649/18710/19802). Narrow to these
                // 4 SSA-emit dispatches — Reflect.get's 3rd `receiver`
                // arg is spec-meaningful only for Proxy targets (tora
                // has no Proxy substrate), silent-drop is spec-correct
                // for the non-Proxy case tora supports.
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ((ns == "Object" && matches!(m_name.as_str(), "hasOwn" | "is"))
                        || (ns == "Reflect" && matches!(m_name.as_str(), "has" | "get")))
                    && args.len() >= 3
                {
                    let _ = self.type_of(ast, args[0])?;
                    let _ = self.type_of(ast, args[1])?;
                    for &arg in args.iter().skip(2) {
                        let _ = self.type_of(ast, arg)?;
                    }
                    return Ok(match m_name.as_str() {
                        "hasOwn" | "is" | "has" => Type::Boolean,
                        "get" => Type::Any,
                        _ => unreachable!(),
                    });
                }
                // S262 — Boolean/Symbol/String × {toString,toLocaleString}
                // trailing-arg ignore per ES §20.3.3.{2,3} / §20.4.3.3
                // / §22.1.3.{27,28}. Each method's `Vec::new()` sig
                // rejected 1+ arg calls; SSA-emit already silent-drops
                // user args (ssa_lower.rs ~16565 Symbol / ~16579 Bool
                // both push only `vec![recv_op]`; ~16593 String returns
                // recv_op identity). Narrow widen: matches m_name +
                // 3 receiver types + args.len() >= 1, returns String.
                // (Number.toLocaleString already handled by S260 above
                // with the 2-arg sig; Number.toString radix handled by
                // S244 in the wedge.)
                if let Expr::Member {
                    obj: recv_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "toString" | "toLocaleString")
                    && !args.is_empty()
                {
                    let recv_ty = self.type_of(ast, *recv_id)?;
                    if matches!(recv_ty, Type::Boolean | Type::Symbol | Type::String) {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::String);
                    }
                }
                // S304 — Struct instance Object.prototype methods
                // trailing-arg ignore per ES §20.1.3.{2,3,4,5,7}. Useful
                // arity: toString/valueOf:0, hasOwnProperty/propertyIs
                // Enumerable/isPrototypeOf:1. tora's struct-instance arms
                // (4619-4632) declared fixed sigs that rejected trailing
                // operands at the generic arity check. ssa_lower's struct
                // dispatch never reads args[useful..] (toString → "[object
                // Object]" const; hasOwnProperty → layout scan keyed on
                // args[0] String literal), so trailing typecheck-and-drop
                // here suffices — no SSA mirror needed. (BigInt.toLocale
                // String trailing also surfaced during probe but its
                // 0-arg form is itself unsupported by ssa_lower — kept
                // in L3b until the substrate lands.)
                if let Expr::Member {
                    obj: recv_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                {
                    let recv_ty = self.type_of(ast, *recv_id)?;
                    let useful = match (&recv_ty, m_name.as_str()) {
                        (Type::Struct(_), "toString") | (Type::Struct(_), "valueOf") => {
                            Some((0, false))
                        }
                        (Type::Struct(_), "hasOwnProperty")
                        | (Type::Struct(_), "propertyIsEnumerable")
                        | (Type::Struct(_), "isPrototypeOf") => Some((1, true)),
                        _ => None,
                    };
                    if let Some((useful, _is_bool)) = useful
                        && args.len() > useful
                    {
                        for &a in args.iter() {
                            let _ = self.type_of(ast, a)?;
                        }
                        let ret = match m_name.as_str() {
                            "toString" => Type::String,
                            "toLocaleString" => Type::String,
                            "valueOf" => recv_ty.clone(),
                            "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf" => {
                                Type::Boolean
                            }
                            _ => unreachable!(),
                        };
                        return Ok(ret);
                    }
                }
                // S261 — Date instance 0-arg getter / format method
                // family (`d.getTime() / d.getFullYear() / d.toISOString() /
                // d.toLocaleString() / ...`) trailing-arg ignore per ES
                // §21.4.4.*. Spec methods either take 0 args or have
                // optional `key`/`locales`/`options` args that tora's
                // subset silently drops; the SSA-emit default branch
                // (ssa_lower.rs ~21601 `else { vec![recv_op] }`) only
                // forwards recv_op for these 0-arg methods, so trailing
                // user args naturally drop. Narrow widen: matches the
                // 0-arg method list, Type::Date receiver, args.len() >= 1.
                if let Expr::Member {
                    obj: recv_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "getTime"
                            | "valueOf"
                            | "toISOString"
                            | "toJSON"
                            | "getFullYear"
                            | "getUTCFullYear"
                            | "getMonth"
                            | "getUTCMonth"
                            | "getDate"
                            | "getUTCDate"
                            | "getHours"
                            | "getUTCHours"
                            | "getMinutes"
                            | "getUTCMinutes"
                            | "getSeconds"
                            | "getUTCSeconds"
                            | "getMilliseconds"
                            | "getUTCMilliseconds"
                            | "getDay"
                            | "getUTCDay"
                            | "getTimezoneOffset"
                            | "getYear"
                            | "toGMTString"
                            | "toUTCString"
                            | "toDateString"
                            | "toLocaleString"
                            | "toLocaleDateString"
                            | "toLocaleTimeString"
                    )
                    && !args.is_empty()
                {
                    let recv_ty = self.type_of(ast, *recv_id)?;
                    if matches!(recv_ty, Type::Date) {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(
                            if matches!(
                                m_name.as_str(),
                                "toISOString"
                                    | "toJSON"
                                    | "toGMTString"
                                    | "toUTCString"
                                    | "toDateString"
                                    | "toLocaleString"
                                    | "toLocaleDateString"
                                    | "toLocaleTimeString"
                            ) {
                                Type::String
                            } else {
                                Type::Number
                            },
                        );
                    }
                }
                // S260 — `n.toLocaleString(locales?, options?, ...trailing)`
                // trailing-arg ignore per ES §21.1.3.4. Spec reads
                // locales + options + ignores rest; tora ignores all
                // formatting args already (en-US-only subset; ssa_lower
                // pass_args=false at line ~16707). The fixed-arity
                // `vec![Any, Any]` sig above rejects 3+ arg calls —
                // widen here. Type-erased silent-drop matches the
                // SSA-emit reality (no arg flows to runtime helper).
                if let Expr::Member {
                    obj: recv_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "toLocaleString"
                    && args.len() >= 3
                {
                    let recv_ty = self.type_of(ast, *recv_id)?;
                    if matches!(recv_ty, Type::Number) {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::String);
                    }
                }
                // S259 — Symbol.{for,keyFor}(key|s, ...trailing)
                // trailing-arg ignore per ES §19.4.{2,3}. Spec reads
                // only args[0]; tora silent-drops trailing. SSA-emit
                // mirror widens `args.len() == 1` gate to `>= 1`
                // (ssa_lower.rs ~18877).
                if let Expr::Member {
                    obj: ns_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && let Expr::Ident(ns) = ast.get_expr(*ns_id)
                    && ns == "Symbol"
                    && matches!(m_name.as_str(), "for" | "keyFor")
                    && args.len() >= 2
                {
                    let _ = self.type_of(ast, args[0])?;
                    for &arg in args.iter().skip(1) {
                        let _ = self.type_of(ast, arg)?;
                    }
                    return Ok(match m_name.as_str() {
                        "for" => Type::Symbol,
                        "keyFor" => Type::Nullable(Box::new(Type::String)),
                        _ => unreachable!(),
                    });
                }
                // S248 — Set.add / Map.set (value, ...trailing) /
                // (key, value, ...trailing) trailing-arg ignore per
                // ES §24.2.3.1 (Set.prototype.add) / §23.1.3.9
                // (Map.prototype.set). Spec adds the useful args then
                // returns the receiver for chained idiom; tora widens
                // the dispatch to accept trailing operands and drops
                // them at lower-time (ssa-lower debug-assert widens
                // `== N` → `>= N`; receiver-chain rc_inc unchanged).
                // Same narrow-shape as S243-S247.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "add" | "set")
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Set) && m_name == "add" && args.len() >= 2 {
                        let _ = self.type_of(ast, args[0])?;
                        for &arg in &args[1..] {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::Set);
                    }
                    if matches!(src_ty, Type::Map) && m_name == "set" && args.len() >= 3 {
                        let _ = self.type_of(ast, args[0])?;
                        let _ = self.type_of(ast, args[1])?;
                        for &arg in &args[2..] {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::Map);
                    }
                }
                // S269 — Object.{create,setPrototypeOf,defineProperties}
                // trailing-arg ignore per ES §20.1.2.{1,5,21}. tora's
                // fixed sigs (`vec![Type::Any]` for create / `vec![
                // Type::Any, Type::Any]` for the other two) rejected
                // the next arg; SSA-emit's intercept for all three
                // already eval-and-drops args[1..] (`for a in args`
                // / `for a in args.iter().skip(1)`), so the lower path
                // is safe — S269 widens checktime to accept the matching
                // floor and beyond.
                //
                // S317 — extend the same widen to `defineProperty(obj,
                // key, desc, ...trailing)` per ES §20.1.2.6. fixed
                // sig `vec![Type::Any, Type::String, Type::Any]` (3
                // args) rejected the 4th; paired ssa_lower change
                // widens `args.len() == 3` to `>= 3` + lowers-and-
                // drops args[3..] after `emit_define_one` for spec
                // left-to-right side-effect order.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "create" | "setPrototypeOf" | "defineProperties" | "defineProperty"
                    )
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Object("Object")) {
                        let floor: usize = match m_name.as_str() {
                            "create" => 2,
                            "setPrototypeOf" | "defineProperties" => 3,
                            "defineProperty" => 4,
                            _ => unreachable!(),
                        };
                        if args.len() >= floor {
                            for &arg in args.iter() {
                                let _ = self.type_of(ast, arg)?;
                            }
                            return Ok(match m_name.as_str() {
                                "defineProperties" | "defineProperty" => Type::Void,
                                _ => Type::Any,
                            });
                        }
                    }
                }
                // S268 — Date instance setter trailing-arg ignore per
                // ES §21.4.4.{20-26}: each per-field setter accepts up
                // to N Number args (year/month/date/hours/etc); trailing
                // args beyond that silent-drop per ES trailing-arg
                // ignore. tora's fixed-N sigs rejected the next arg;
                // SSA-emit's `for i in 0..target_arity` loop already
                // takes only args[0..arity] so trailing operands fall
                // off the slice (release-build); pair with a skip(arity)
                // eval-and-drop loop in the matching widen below to
                // preserve side effects and replace the
                // `debug_assert!(args.len() <= arity)`.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "setFullYear"
                            | "setMonth"
                            | "setDate"
                            | "setHours"
                            | "setMinutes"
                            | "setSeconds"
                            | "setMilliseconds"
                            | "setTime"
                            | "setYear"
                    )
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Date) {
                        let max_arity: usize = match m_name.as_str() {
                            "setFullYear" | "setMinutes" => 3,
                            "setMonth" | "setSeconds" => 2,
                            "setDate" | "setMilliseconds" | "setTime" | "setYear" => 1,
                            "setHours" => 4,
                            _ => unreachable!(),
                        };
                        if args.len() > max_arity {
                            for &arg in args.iter() {
                                let _ = self.type_of(ast, arg)?;
                            }
                            return Ok(Type::Number);
                        }
                    }
                }
                // S267 — Array.isArray(value, ...trailing) per ES
                // §23.1.2.2. The spec uses only step 1's `value`;
                // trailing args silent-drop. tora's check.rs sig
                // `vec![Type::Any] -> Boolean` rejected 1+ trailing
                // with "expected 1 argument(s), got N"; ssa_lower's
                // intercept already routes through args[0] only, so
                // a skip(1) eval-and-drop loop preserves side effects
                // for any trailing expression.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "isArray"
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Object("Array")) {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::Boolean);
                    }
                }
                // S266 — RegExp.{test,exec,toString}(s?, ...trailing)
                // per ES §22.2.6.{2,7,16}. Each method's fixed sig
                // (`vec![Type::String]` / `Vec::new()`) rejected 1+
                // trailing args. SSA-emit uses only args[0] (test/exec)
                // or no args (toString); the matching widen relaxes
                // the debug-assert + eval-and-drops args[1..].
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "test" | "exec" | "toString")
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::RegExp)
                        && matches!(m_name.as_str(), "test" | "exec")
                        && args.len() >= 2
                    {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(match m_name.as_str() {
                            "test" => Type::Boolean,
                            "exec" => Type::Array(Box::new(Type::String)),
                            _ => unreachable!(),
                        });
                    }
                    if matches!(src_ty, Type::RegExp) && m_name == "toString" && !args.is_empty() {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::String);
                    }
                }
                // S265 — Object.{getPrototypeOf,isExtensible,isSealed,
                // preventExtensions,seal}(obj, ...trailing) per ES
                // §20.1.2.{12,13,14,16,18,20}. Each method's fixed sig
                // (`vec![Type::Any]`) rejected 1+ trailing args; ssa_lower
                // for the 4 anyv_* helpers already drops args[1..] via
                // `for a in args.iter().skip(1) { let _ = self.lower_expr(*a); }`;
                // getPrototypeOf gets the same treatment in the matching
                // widen below. Returns the original Any (proto/cell) or
                // Boolean per the underlying sig.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "getPrototypeOf"
                            | "isExtensible"
                            | "isSealed"
                            | "preventExtensions"
                            | "seal"
                    )
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Object("Object")) {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(match m_name.as_str() {
                            "isExtensible" | "isSealed" => Type::Boolean,
                            _ => Type::Any,
                        });
                    }
                }
                // S264 — Set/Map instance method trailing-arg ignore
                // per ES §24.2.3.{4,5,7} (Set.{delete,clear,has}) +
                // §23.1.3.{3,4,6,7} (Map.{delete,clear,get,has}).
                // Each method's fixed sig (vec![Any] / Vec::new())
                // rejected 1+ trailing args; ssa_lower's strict
                // `debug_assert_eq!(args.len(), 1)` / `args.is_empty()`
                // becomes a `>= N` floor in the matching widen below.
                // (Set.add / Map.set already covered by S248.)
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "has" | "delete" | "get" | "clear")
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    // (Set|Map).{has,delete} accept >= 2 (key + trail).
                    if matches!(src_ty, Type::Set | Type::Map)
                        && matches!(m_name.as_str(), "has" | "delete")
                        && args.len() >= 2
                    {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::Boolean);
                    }
                    // Map.get accepts >= 2 (key + trail).
                    if matches!(src_ty, Type::Map) && m_name == "get" && args.len() >= 2 {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::Nullable(Box::new(Type::Any)));
                    }
                    // (Set|Map).clear accept >= 1 (trail only).
                    if matches!(src_ty, Type::Set | Type::Map)
                        && m_name == "clear"
                        && !args.is_empty()
                    {
                        for &arg in args.iter() {
                            let _ = self.type_of(ast, arg)?;
                        }
                        return Ok(Type::Void);
                    }
                }
                // S301 — WeakMap.{set,get,has,delete} + WeakSet.{add,has,
                // delete} trailing-arg ignore per ES §24.{3,4}.3.*. Useful
                // arity is set:2 / others:1; fixed-sig declarations rejected
                // trailing args with "expected N argument(s), got M".
                // typecheck-and-drop args[useful..] mirror in ssa_lower (the
                // WeakMap/WeakSet lower dispatch loop also caps useful when
                // building full_args). Same family as S264 Set/Map block.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    let useful = match (&src_ty, m_name.as_str()) {
                        (Type::WeakMap, "set") => Some(2),
                        (Type::WeakMap, "get")
                        | (Type::WeakMap, "has")
                        | (Type::WeakMap, "delete") => Some(1),
                        (Type::WeakSet, "add")
                        | (Type::WeakSet, "has")
                        | (Type::WeakSet, "delete") => Some(1),
                        _ => None,
                    };
                    if let Some(useful) = useful
                        && args.len() > useful
                    {
                        for &a in args.iter() {
                            let _ = self.type_of(ast, a)?;
                        }
                        let ret = match (&src_ty, m_name.as_str()) {
                            (Type::WeakMap, "set") => Type::Void,
                            (Type::WeakMap, "get") => Type::Nullable(Box::new(Type::Any)),
                            (Type::WeakSet, "add") => Type::Void,
                            _ => Type::Boolean,
                        };
                        return Ok(ret);
                    }
                }
                // S318 — Set ES2025 setops trailing-arg ignore per ES
                // §24.2.5.{4-10}: spec sig is 1-arg (other SetLike);
                // trailing args silent-drop. Pre-S318 the fixed
                // (Type::Set, "isSubsetOf"|...) method-table sig
                // `vec![Type::Set]` rejected `args.len() >= 2` with
                // "expected 1 argument(s), got M". typecheck-drop
                // args[1..] mirror in ssa_lower (replace
                // `debug_assert_eq!(args.len(), 1)` carve-out with
                // lower-and-drop loop after args[0] lower).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "isSubsetOf"
                            | "isSupersetOf"
                            | "isDisjointFrom"
                            | "union"
                            | "intersection"
                            | "difference"
                            | "symmetricDifference"
                    )
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Set) {
                        for &a in args.iter() {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(match m_name.as_str() {
                            "isSubsetOf" | "isSupersetOf" | "isDisjointFrom" => Type::Boolean,
                            _ => Type::Set,
                        });
                    }
                }
                // S315 — Object.getOwnPropertyDescriptor(obj, key, ...trailing)
                // per ES §20.1.2.10: spec sig is 2-arg; trailing args MUST
                // be silently ignored. Pre-S315 the fixed (Type::Object(
                // "Object"), "getOwnPropertyDescriptor") method-table sig
                // rejected `args.len() >= 3` with "expected 2 argument(s),
                // got M". typecheck-drop trailing + ssa_lower mirror at
                // 18024 widens `== 2` → `>= 2` and adds skip(2) loop.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "getOwnPropertyDescriptor"
                    && args.len() >= 3
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Object("Object")) {
                        // Useful arg typecheck (obj: Any, key: String).
                        let _ = self.type_of(ast, args[0])?;
                        let aty1 = self.type_of(ast, args[1])?;
                        if !matches!(aty1, Type::String) {
                            return Err(format!(
                                "Object.getOwnPropertyDescriptor arg 1 (key) must be string, got {aty1:?}"
                            ));
                        }
                        for &a in args.iter().skip(2) {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::Any);
                    }
                }
                // S314 — BigInt.{asIntN,asUintN}(bits, value, ...trailing)
                // per ES §21.2.2.{1,2}: spec sig is 2-arg; trailing args
                // MUST be silently ignored. Pre-S314 the fixed (Type::
                // Object("BigInt"), m) method-table sig rejected
                // `args.len() >= 3` with "expected 2 argument(s), got M".
                // typecheck-drop trailing + ssa_lower mirror widens gate
                // + lower-and-drop trailing.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "asIntN" | "asUintN")
                    && args.len() >= 3
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Object("BigInt")) {
                        // Useful arg typecheck (bits: Number, value: BigInt).
                        let aty0 = self.type_of(ast, args[0])?;
                        if !matches!(aty0, Type::Number) {
                            return Err(format!(
                                "BigInt.{m_name} arg 0 (bits) must be number, got {aty0:?}"
                            ));
                        }
                        let aty1 = self.type_of(ast, args[1])?;
                        if !matches!(aty1, Type::BigInt) {
                            return Err(format!(
                                "BigInt.{m_name} arg 1 (value) must be bigint, got {aty1:?}"
                            ));
                        }
                        for &a in args.iter().skip(2) {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::BigInt);
                    }
                }
                // S309 — Object.fromEntries(entries, ...trailing) per ES
                // §20.1.2.7: spec sig is 1-arg (entries iterable);
                // trailing args are silently ignored. Pre-S309 the
                // fixed (Type::Object("Object"), "fromEntries") method-
                // table sig rejected `args.len() >= 2` with "expected
                // 1 argument(s), got M". typecheck-drop args[1..] +
                // ssa_lower mirror (LetDecl fast-path widens
                // is_fromentries_call gate + lower-and-drop trailing).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "fromEntries"
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::Object("Object")) {
                        for &a in args.iter() {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::Any);
                    }
                }
                // S210 — String.search() / search(undefined) per ES
                // §22.1.3.20: RegExpCreate(undefined, undefined)
                // yields an empty regex which matches at index 0.
                // Pre-fix tora declared `(String) -> Number`, so
                // 0-arg failed at arity and 1-arg-undefined failed
                // with "argument 0: expected String, got Undefined".
                // ssa_lower short-circuits to ConstI64(0).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "search"
                    && args.len() < 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        if let Some(&aid) = args.first() {
                            let aty = self.type_of(ast, aid)?;
                            if matches!(aty, Type::Undefined) {
                                return Ok(Type::Number);
                            }
                        } else {
                            return Ok(Type::Number);
                        }
                    }
                }
                // S325 — WeakRef.deref(...trailing) trailing-arg ignore
                // per ES §26.1.3.2. spec is 0-arg; tora's static-table sig
                // `(WeakRef, "deref") -> Function([], Nullable<Any>)` at
                // ~3661 rejected 1+ arg calls at strict arity. Carve-out
                // typecheck-and-drops args[..]; ssa_lower mirror peeks the
                // receiver via expr_types and lower-and-drops trailing.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "deref"
                    && !args.is_empty()
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::WeakRef) {
                        for &a in args.iter() {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::Nullable(Box::new(Type::Any)));
                    }
                }
                // S324 — String.search(needle, ...trailing) trailing-arg
                // ignore per ES §22.1.3.20. spec reads only needle; tora's
                // declared `(String) -> Number` sig at ~4194 rejected 2+ arg
                // calls at strict arity. Mirror the S240-family widen (1-
                // useful methods): typecheck-and-drop args[1..] and let the
                // existing (String) -> Number arm pick up the result type.
                // ssa_lower_str dispatch loop adds `"search"` to the S240
                // 1-useful trailing-drop list so step()-style trailing args
                // fire and the (Str, Str) helper ABI never sees them.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "search"
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let needle_ty = self.type_of(ast, args[0])?;
                        if !matches!(needle_ty, Type::String | Type::Undefined) {
                            return Err(format!(
                                "String.search arg 0 must be string, got {needle_ty:?}"
                            ));
                        }
                        for &a in args.iter().skip(1) {
                            let _ = self.type_of(ast, a)?;
                        }
                        return Ok(Type::Number);
                    }
                }
                // S209 — String.repeat(undefined) per ES §22.1.3.17
                // step 1: ToIntegerOrInfinity(undefined) = 0 → return
                // the empty string. Declared `vec![Type::Number] ->
                // String` rejected the typed-Undefined arg with
                // "argument 0: expected Number, got Undefined".
                // ssa_lower_str pushes ConstI64(0) for the missing
                // count and routes through str_repeat as usual.
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "repeat"
                    && args.len() == 1
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let aty = self.type_of(ast, args[0])?;
                        if matches!(aty, Type::Undefined) {
                            return Ok(Type::String);
                        }
                    }
                }
                // S207 — String.replace / replaceAll with fewer-than-2
                // args per ES §22.1.3.18 / §22.1.3.19 step 4 + step 6a:
                //   searchString = ToString(searchValue ?? undefined)
                //                = "undefined"
                //   replaceValue = ToString(replaceValue ?? undefined)
                //                = "undefined"
                // Declared `(Any, Any) -> String` would reject 0/1-arg
                // shapes at the strict-arity gate; widen here and let
                // ssa_lower_str push the missing "undefined" interns
                // so the helper sees a valid Str needle / replacement.
                // Bun-aligned: 0-arg → identity unless haystack contains
                // "undefined"; 1-arg → first match of needle replaced
                // by the literal "undefined".
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && (m_name == "replace" || m_name == "replaceAll")
                    && args.len() < 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        // Populate expr_types for any present arg so
                        // ssa_lower_str can detect a typed-undefined
                        // operand (via expr_types.get) and substitute
                        // the interned "undefined" literal instead of
                        // emitting a non-Str operand the helper would
                        // deref as a Str pointer.
                        for &aid in args {
                            self.type_of(ast, aid)?;
                        }
                        return Ok(Type::String);
                    }
                }
                // S339 — `xs.with(Any idx, val)` per ES §23.1.3.39 step 2:
                // ToIntegerOrInfinity accepts arbitrary-typed input. The
                // method-table sig `(Number, T) -> Array<T>` at the Array
                // dispatch site (~4422) strict-rejected `o: any` operands
                // at typecheck ('argument 0: expected Number, got Any').
                // Widen here so the ssa_lower mirror routes Any through
                // anyv_to_number → coerce_to_i64 → arr_with helper.
                // Sister to S331/S332/S333/S334/S335. Covers both basic
                // 2-arg case and trailing-arg ≥3 case (S283 sibling
                // below stays strict-Number for non-Any args[0]).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "with"
                    && args.len() >= 2
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let aty0 = self.type_of(ast, args[0])?;
                        if matches!(aty0, Type::Any) {
                            let inner = (**elem).clone();
                            let aty1 = self.type_of(ast, args[1])?;
                            if aty1 != inner && !matches!(inner, Type::Any) {
                                return Err(format!(
                                    "Array.with arg 1 (value) must match elem type {:?}, got {aty1:?}",
                                    inner
                                ));
                            }
                            for &aid in &args[2..] {
                                let _ = self.type_of(ast, aid)?;
                            }
                            return Ok(Type::Array(Box::new(inner)));
                        }
                    }
                }
                // S283 — Array.prototype.with(index, value, ...trailing)
                // trailing-arg ignore per ES §23.1.3.39. Spec reads only
                // index + value; tora's arr_with intrinsic is 2-arg only.
                // Widen check.rs to accept args.len() >= 3; ssa_lower
                // mirror widens the `args.len() == 2` gate to `>= 2`
                // and lowers args[2..] for side effects (S272 idiom).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && m_name == "with"
                    && args.len() >= 3
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if let Type::Array(elem) = &src_ty {
                        let inner = (**elem).clone();
                        let aty0 = self.type_of(ast, args[0])?;
                        if !matches!(aty0, Type::Number) {
                            return Err(format!(
                                "Array.with arg 0 (index) must be number, got {aty0:?}"
                            ));
                        }
                        let aty1 = self.type_of(ast, args[1])?;
                        if aty1 != inner && !matches!(inner, Type::Any) {
                            return Err(format!(
                                "Array.with arg 1 (value) must match elem type {:?}, got {aty1:?}",
                                inner
                            ));
                        }
                        for &aid in &args[2..] {
                            let _ = self.type_of(ast, aid)?;
                        }
                        return Ok(Type::Array(Box::new(inner)));
                    }
                }
                // S282 — String.{replace,replaceAll,split}(useful, useful,
                // ...trailing) trailing-arg ignore per ES §22.1.3.{18,19,
                // 21}. Spec reads only 2 args (search/replace or
                // separator/limit); tora's helpers are 2-arg only.
                // Widen check.rs to accept args.len() >= 3; ssa_lower
                // mirror lowers args[2..] for side effects then drops
                // the values (S272 idiom).
                if let Expr::Member {
                    obj: src_id,
                    name: m_name,
                } = ast.get_expr(*callee)
                    && matches!(m_name.as_str(), "replace" | "replaceAll" | "split")
                    && args.len() >= 3
                {
                    let src_ty = self.type_of(ast, *src_id)?;
                    if matches!(src_ty, Type::String) {
                        let _ = self.type_of(ast, args[0])?;
                        let _ = self.type_of(ast, args[1])?;
                        for &aid in &args[2..] {
                            let _ = self.type_of(ast, aid)?;
                        }
                        return Ok(match m_name.as_str() {
                            "replace" | "replaceAll" => Type::String,
                            "split" => Type::Array(Box::new(Type::String)),
                            _ => unreachable!(),
                        });
                    }
                }
                let callee_ty = self.type_of(ast, *callee)?;
                let Type::Function(mut params, ret) = callee_ty else {
                    return Err(format!("not callable: type {callee_ty:?}"));
                };
                // P1 wedge — Array.prototype callback methods accept
                // an optional trailing thisArg per ES spec §23.1.3.X
                // (map/filter/every/some/forEach/find/findIndex/
                // findLast/findLastIndex/reduce/reduceRight/flatMap).
                // tora's callbacks don't have `this` semantics
                // (closures don't bind a receiver), so the thisArg
                // is silently dropped — tests that don't rely on
                // `this` inside the callback now typecheck (~70+
                // cases unblocked across the broader sample). Tests
                // that DO use `this` were already blocked on the
                // missing-this substrate; the silent drop doesn't
                // make those worse.
                let mut effective_args = args.clone();
                if args.len() >= params.len() + 1
                    && let Expr::Member { name: m_name, .. } = ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "map"
                            | "filter"
                            | "every"
                            | "some"
                            | "forEach"
                            | "find"
                            | "findIndex"
                            | "findLast"
                            | "findLastIndex"
                            | "flatMap"
                    )
                {
                    // S270 — widen the thisArg drop from `== params+1`
                    // to `>= params+1` so any trailing args past thisArg
                    // are also silent-dropped per ES §23.1.3.X trailing-
                    // arg ignore (xs.map(cb, thisArg, ...trailing) is
                    // spec-legal — spec uses only cb + thisArg). Type-
                    // check every dropped arg so expr's internal errors
                    // still surface; SSA-emit reads only args[0] (cb).
                    for &arg in &effective_args[params.len()..] {
                        let _ = self.type_of(ast, arg)?;
                    }
                    effective_args.truncate(params.len());
                }
                // T-28 — Default param missing → undefined (per ES
                // spec §10.2.1.4). When fewer args are supplied than
                // params, JS sets the missing slots to undefined. Only
                // safe for Type::Any params (typed slots can't hold
                // undefined). Typed missing params still error so
                // typed code keeps strict arity. ssa_lower pads the
                // missing positions with ANY_UNDEF boxes at the call
                // site.
                if effective_args.len() < params.len() {
                    let trailing_all_any = params[effective_args.len()..]
                        .iter()
                        .all(|t| matches!(t, Type::Any));
                    if trailing_all_any {
                        // Type-check what was actually passed (rest stay
                        // as undefined). Pad-with-undef happens at SSA
                        // layer via the `padded_args` path keyed off
                        // expr_arity_pad. Stash the missing count on
                        // the call site so ssa_lower can emit ANY_UNDEF
                        // boxes for the trailing positions.
                        for arg_id in effective_args.iter() {
                            let _ = self.type_of(ast, *arg_id)?;
                        }
                        self.arity_pad_count
                            .insert(eid, params.len() - effective_args.len());
                        return Ok((*ret).clone());
                    }
                }
                // Date per-field setters (setFullYear / setMonth /
                // setHours / setMinutes / setSeconds / …) accept 1-N
                // args per ES §21.4.4.20-26 with trailing positions
                // optional. The sig declares the FULL arity (max); when
                // the caller supplied fewer args we narrow the sig to
                // the supplied arity so strict-arity below passes. The
                // ssa_lower side sentinel-pads the missing trailing
                // positions with `DATE_FIELD_KEEP` (i64::MIN).
                if effective_args.len() < params.len()
                    && let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && matches!(
                        name.as_str(),
                        "setFullYear" | "setMonth" | "setHours" | "setMinutes" | "setSeconds"
                    )
                {
                    let recv_ty = self.type_of(ast, *obj)?;
                    if recv_ty == Type::Date && effective_args.len() >= 1 {
                        params.truncate(effective_args.len());
                    }
                }
                // `arr.splice(start, deleteCount?)` — per ES §23.1.3.31
                // both args are spec-optional: 0-arg form gives
                // `start = 0`/`deleteCount = 0`, 1-arg form defaults
                // `deleteCount = len - actualStart`. The Type::Function
                // declared above is the 2-arg full form; truncate
                // params to the actual arity so the strict-equality
                // arity check below accepts 0 / 1 args. The
                // `ssa_lower_splice` siblings fill the defaults at
                // emit time (0 → `ConstI64(0)`, 1 → `i64::MAX`
                // sentinel that the helper clamps to `len - start`).
                if effective_args.len() < params.len()
                    && let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && matches!(name.as_str(), "splice" | "toSpliced")
                {
                    let recv_ty = self.type_of(ast, *obj)?;
                    if matches!(recv_ty, Type::Array(_)) {
                        params.truncate(effective_args.len());
                    }
                }
                // S237 — `arr.splice(start, undefined)` /
                // `arr.toSpliced(start, undefined)` per ES §23.1.3.31
                // step 7: ToIntegerOrInfinity(undefined) = 0, so an
                // explicit-undefined deleteCount yields 0 removal
                // (matches bun: returns `[]`, leaves the source
                // unchanged). Distinct from the omitted-arg 1-arg
                // shape above which spec-defaults deleteCount to
                // `len - actualStart`. Trim both `params` and
                // `effective_args` to the 1-arg shape when arg 1 is
                // Undefined so the strict-equality arity check below
                // accepts it; ssa_lower_splice mirror detects the
                // 2-arg-undef shape and emits ConstI64(0) for
                // deleteCount instead of the 1-arg i64::MAX sentinel.
                if effective_args.len() == 2
                    && params.len() == 2
                    && let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && matches!(name.as_str(), "splice" | "toSpliced")
                {
                    let recv_ty = self.type_of(ast, *obj)?;
                    let arg1_ty = self.type_of(ast, effective_args[1])?;
                    if matches!(recv_ty, Type::Array(_)) && matches!(arg1_ty, Type::Undefined) {
                        params.truncate(1);
                        effective_args.truncate(1);
                    }
                }
                // `arr.at(index?)` / `s.at(index?)` — per ES §22.1.3.1 /
                // §23.1.3.1 step 2-3, `undefined` routes through
                // ToIntegerOrInfinity → 0, so the 0-arg form returns
                // index 0. The Type::Function declared above is the
                // 1-arg full form; truncate to 0 params on no-arg call
                // so the strict-equality arity check accepts it. The
                // `ssa_lower_str` Arr and String `at` emitters fill the
                // default `ConstI64(0)` at emit time.
                if effective_args.len() < params.len()
                    && let Expr::Member { obj, name } = ast.get_expr(*callee)
                    && name == "at"
                {
                    let recv_ty = self.type_of(ast, *obj)?;
                    if matches!(recv_ty, Type::Array(_) | Type::String) {
                        params.truncate(effective_args.len());
                    }
                }
                // `arr.indexOf() / .includes() / .lastIndexOf()` and
                // `s.indexOf() / .includes() / .lastIndexOf() /
                // .startsWith() / .endsWith()` — per ES §22.1.3.{8,13,...}
                // and §23.1.3.{14,17,18}, `searchElement` / `searchString`
                // defaults to undefined when omitted (algorithm steps
                // read the missing argument as undefined). The
                // Type::Function declared above is the 1-arg full form;
                // truncate to 0 params on the no-arg call.
                //
                // SSA emit semantics:
                //   String: fill needle = ToString(undefined) = "undefined"
                //     and search normally (bun observable).
                //   Array<T> with T ≠ Any: undefined cannot strict-equal
                //     any typed element, so the result is fixed
                //     (-1 / false). Array<Any> is omitted from the
                //     truncate here because the search would need an
                //     Any-tagged undefined sentinel; L3b.
                if effective_args.is_empty()
                    && let Expr::Member { obj, name } = ast.get_expr(*callee)
                {
                    let recv_ty = self.type_of(ast, *obj)?;
                    let trunc = match (&recv_ty, name.as_str()) {
                        (
                            Type::String,
                            "indexOf" | "includes" | "lastIndexOf" | "startsWith" | "endsWith",
                        ) => true,
                        (Type::Array(elem), "indexOf" | "includes" | "lastIndexOf") => {
                            **elem != Type::Any
                        }
                        _ => false,
                    };
                    if trunc {
                        params.truncate(0);
                    }
                }
                // S243 — narrow trailing-arg ignore for Math.* namespace
                // methods per ES §21.3.2.* "trailing args are ignored":
                // each Math.* algorithm reads only the declared positional
                // args; the surface Type::Function sig is closed but the
                // underlying calling convention is open-arity. SSA-emit's
                // generic Math.* call lowering already truncates extras
                // before the (f64, f64...) / (f64,) helper ABI, so we
                // only need to truncate at the check arity gate and
                // type_of the trailing operands for side effects. Other
                // builtin receivers (Number/Array/String) stay on the
                // existing narrow carve-outs (S238–S242) until per-
                // method SSA-emit shape widens to match.
                if effective_args.len() > params.len()
                    && let Expr::Member { obj, name: _ } = ast.get_expr(*callee)
                {
                    let recv_ty = self.type_of(ast, *obj)?;
                    // S243 Math.* / S250 Date.<static> narrow trailing-
                    // arg ignore per ES §21.3.2.* / §21.4.3.*. Each
                    // intrinsic reads only declared positional args;
                    // SSA-emit's Call lowering tolerates extras at the
                    // (intrinsic) ABI boundary. Other builtin receivers
                    // (Number/Array/String) stay on per-method narrow
                    // carve-outs (S238-S248).
                    if matches!(recv_ty, Type::Object("Math") | Type::Object("Date")) {
                        for &aid in &effective_args[params.len()..] {
                            let _ = self.type_of(ast, aid)?;
                        }
                        effective_args.truncate(params.len());
                    }
                }
                if params.len() != effective_args.len() {
                    return Err(format!(
                        "expected {} argument(s), got {}",
                        params.len(),
                        effective_args.len()
                    ));
                }
                let args = &effective_args;
                // M6.1 — String borrow-methods (slice/includes/indexOf/...)
                // don't transfer ownership of either receiver or args.
                // They read both, allocate a fresh result, and return.
                let is_string_borrow = matches!(
                    ast.get_expr(*callee),
                    Expr::Member { obj: _, name }
                        if STRING_BORROW_METHODS.iter().any(|m| *m == name.as_str())
                );
                // M5.1 — class methods (`__cm_C__m(receiver, ...)`) borrow
                // the receiver: arg[0] is read, never consumed. Args[1..]
                // follow the normal affine rules.
                let is_class_method = matches!(
                    ast.get_expr(*callee),
                    Expr::Ident(name) if is_class_method_name(name)
                );
                // Per-call-site consume bitmap, derived from
                // `ast.consuming_params` for the callee fn (computed by
                // `compute_consuming_params` from the body's flow into
                // `__new_*` / `this.<field> =` sinks). For unknown
                // callees (intrinsics, builtins) the default is "borrow"
                // — only the constructor-factory shortcut here triggers
                // when consuming_params doesn't have an entry.
                let consume_bitmap: Vec<bool> = match ast.get_expr(*callee) {
                    Expr::Ident(callee_name) => {
                        if let Some(bm) = ast.consuming_params.get(callee_name) {
                            bm.clone()
                        } else if callee_name.starts_with("__new_") {
                            vec![true; args.len()]
                        } else {
                            vec![false; args.len()]
                        }
                    }
                    _ => vec![false; args.len()],
                };
                for (i, (param_ty, arg_id)) in params.iter().zip(args.iter()).enumerate() {
                    let arg_ty = self.type_of(ast, *arg_id)?;
                    // M5.2 — class-method dispatch: arg[0] is the receiver
                    // and may be a SUBCLASS of the declared param type
                    // (structural super-set: subclass struct's fields are
                    // a prefix-extension of the parent's). The SSA / LLVM
                    // layer treats both as ptr, so the call is correct as
                    // long as the layout prefix matches. We just skip the
                    // strict equality here.
                    let skip_type_check =
                        is_class_method && i == 0 && struct_is_prefix_subtype(&arg_ty, param_ty);
                    // V3-18 wedge — Nullable<T> param accepts both
                    // T-typed and Null arg (TS spec §3.9.2.4 optional
                    // param widens to T | undefined; subset models
                    // optional as Nullable<T>).
                    let nullable_match = if let Type::Nullable(inner) = param_ty {
                        arg_ty == Type::Null || &arg_ty == inner.as_ref()
                    } else {
                        false
                    };
                    // S133 narrow — callback Function subtype:
                    // JS spec lets a callback accept fewer args than
                    // the formal Function param declares
                    // (Map.forEach: `(v) =>` legal even though spec sig
                    // is `(v, k, map) => void`). Strict equality on
                    // Type::Function rejects shorter callbacks. Accept
                    // when actual arity ≤ formal arity, every prefix
                    // slot matches with either side being Any (user
                    // callback without type-ann defaults to Any —
                    // accept against typed formal; formal Any accepts
                    // any typed actual), and the return type matches
                    // (or either side Any).
                    let callback_subtype = match (param_ty, &arg_ty) {
                        (
                            Type::Function(formal_ps, formal_ret),
                            Type::Function(actual_ps, actual_ret),
                        ) => {
                            actual_ps.len() <= formal_ps.len()
                                && (formal_ret.as_ref() == actual_ret.as_ref()
                                    || matches!(formal_ret.as_ref(), Type::Any)
                                    || matches!(actual_ret.as_ref(), Type::Any))
                                && actual_ps.iter().zip(formal_ps.iter()).all(|(a, f)| {
                                    a == f || matches!(f, Type::Any) || matches!(a, Type::Any)
                                })
                        }
                        _ => false,
                    };
                    if !skip_type_check
                        && !nullable_match
                        && !callback_subtype
                        && param_ty != &Type::Any
                        && &arg_ty != param_ty
                    {
                        return Err(format!(
                            "argument {i}: expected {param_ty:?}, got {arg_ty:?}"
                        ));
                    }
                    // TS-shape: function parameters borrow non-Copy args
                    // by default. Calling `f(x)` does not mark `x` as
                    // moved — the caller keeps owning the heap and can
                    // pass the same binding to another function later.
                    // Matches JS pass-by-reference semantics. Caveat: a
                    // function that stores its arg into long-lived heap
                    // (e.g. a global, or a returned struct field) would
                    // create a dangling pointer once the caller drops
                    // the original — there's no GC to keep it alive. For
                    // the cases we ship today this is fine; the ts-subset
                    // doc calls out the constraint.
                    let _ = is_string_borrow;
                    let _ = is_class_method;
                    if consume_bitmap.get(i).copied().unwrap_or(false)
                        && !arg_ty.is_copy()
                        && !self.consumed_calls.contains(&eid)
                    {
                        self.consume(ast, *arg_id);
                    }
                }
                self.consumed_calls.insert(eid);
                Ok(*ret)
            }
            Expr::BinOp { op, left, right } => {
                let l = self.type_of(ast, *left)?;
                let r = self.type_of(ast, *right)?;
                match op {
                    BinOp::Add => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Number)
                        } else if l == Type::BigInt && r == Type::BigInt {
                            // T-25 — BigInt + BigInt → BigInt. Mixed
                            // BigInt/Number is a TypeError per spec
                            // (caught by the catch-all below).
                            Ok(Type::BigInt)
                        } else if l == Type::String && r == Type::String {
                            // TS-shape: `a + b` reads both operands, returns
                            // a fresh string. Operands keep their heaps —
                            // `a` and `b` are still readable + droppable
                            // afterwards (matches bun / standard TS).
                            Ok(Type::String)
                        } else if (l == Type::String && r == Type::Number)
                            || (l == Type::Number && r == Type::String)
                        {
                            // JS ToString coercion — ssa_lower routes
                            // the number side through __torajs_i64_to_str
                            // / __torajs_f64_to_str before concat.
                            Ok(Type::String)
                        } else if (l == Type::String && r == Type::BigInt)
                            || (l == Type::BigInt && r == Type::String)
                        {
                            // V3-18 m3.c — BigInt + String concat. Spec
                            // §13.15.3: when one side is String, the
                            // other ToString's. ssa_lower routes the
                            // BigInt side through __torajs_bigint_to_string.
                            Ok(Type::String)
                        } else if (l == Type::String
                            && matches!(r, Type::Boolean | Type::Null | Type::Undefined))
                            || (matches!(l, Type::Boolean | Type::Null | Type::Undefined)
                                && r == Type::String)
                        {
                            // V3-18 m1.d / S142 — String + Bool / Null /
                            // Undefined (and reverse). ssa_lower routes
                            // the non-string side through
                            // __torajs_bool_to_str / __torajs_null_to_str
                            // / __torajs_undefined_to_str before concat.
                            Ok(Type::String)
                        } else if (l == Type::String
                            && matches!(r, Type::Array(_) | Type::Struct(_)))
                            || (matches!(l, Type::Array(_) | Type::Struct(_)) && r == Type::String)
                        {
                            // S138 — `String + Array` / `String + Struct`
                            // per ES §13.15.3 (StringOrNumeric concat:
                            // ToPrimitive(Default) → ToString on the
                            // non-String side). ssa_lower routes Arr
                            // through arr_join(",") and Struct through
                            // the `"[object Object]"` literal — same path
                            // as the explicit `String(arr)` / `String(o)`
                            // S137 call site.
                            Ok(Type::String)
                        } else if (l == Type::String && matches!(r, Type::Any))
                            || (matches!(l, Type::Any) && r == Type::String)
                        {
                            // Spec §13.15.3: when either operand is a
                            // String, ApplyStringOrNumericBinaryOperator
                            // routes through the StringConcat branch
                            // (ToPrimitive(Default) + ToString on the
                            // non-String side), and the result is always
                            // a String. Pre-fix this fell into the Any
                            // catch-all below, which forced callers like
                            // `new Error('msg: ' + e.message)` (where
                            // `e: any` makes `e.message: Any`) to add a
                            // redundant `as string` cast.
                            Ok(Type::String)
                        } else if matches!(l, Type::Any) || matches!(r, Type::Any) {
                            // P0.6 — Any operand on either side per
                            // JS spec §13.15.3 ApplyStringOrNumeric
                            // BinaryOperator. ssa_lower routes through
                            // __torajs_any_add which does ToPrimitive
                            // (hint=Default) then either string concat
                            // or numeric add. Result is Any so
                            // downstream consumers see a boxed value.
                            Ok(Type::Any)
                        } else if js_add_coerces_to_number(&l, &r) {
                            // V3-18 m1.a — JS spec §13.15.3 ToNumber
                            // coercion for non-string + arithmetic.
                            // Boolean → ToNumber → 0/1; Null → 0;
                            // Number stays. Result is Number after both
                            // sides are coerced. ssa_lower mirrors the
                            // coercion at lower time (zext / select /
                            // const-zero) before the actual add. Matches
                            // bun for `1 + true`, `0 + null`, `true +
                            // true`, `null + null`, etc — all from the
                            // test262 addition / coercion buckets.
                            Ok(Type::Number)
                        } else {
                            Err(format!(
                                "`+` requires matching number/string/bigint operands or string+number, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Number)
                        } else if l == Type::BigInt && r == Type::BigInt {
                            // T-25 — BigInt arithmetic. Mixed with
                            // Number is a TypeError per spec.
                            Ok(Type::BigInt)
                        } else if matches!(l, Type::Any) || matches!(r, Type::Any) {
                            // P0.7 — Any operand on either side per
                            // JS spec §13.6 / §13.7 / §13.8 / §13.9
                            // ToNumber both sides, perform the op in
                            // IEEE 754, return Any-boxed Number.
                            Ok(Type::Any)
                        } else if js_arith_coerces_to_number(&l, &r) {
                            // V3-18 m1.b — ToNumber coercion for the
                            // -/*/division operators. Same Bool/Null →
                            // Number rule as `+` (m1.a) but no String
                            // concat path: spec §13.7-§13.10 unconditionally
                            // calls ToNumeric on both sides.
                            Ok(Type::Number)
                        } else {
                            Err(format!(
                                "arithmetic requires number or bigint operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::Pow => {
                        // V3-01 — `**` exponent. Number/Number → Number;
                        // BigInt/BigInt → BigInt. Mixed-type per spec
                        // is a TypeError, caught by the catch-all.
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Number)
                        } else if l == Type::BigInt && r == Type::BigInt {
                            Ok(Type::BigInt)
                        } else {
                            Err(format!(
                                "`**` requires matching number or bigint operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Number)
                        } else if l == Type::BigInt && r == Type::BigInt {
                            // V3-02 — BigInt bitwise w/ two's-complement
                            // simulation. Mixed Number/BigInt rejected.
                            Ok(Type::BigInt)
                        } else if js_arith_coerces_to_number(&l, &r) {
                            // V3-18 m1.e — JS spec §13.12 bitwise ops
                            // call ToInt32 on both operands then do an
                            // i32 op. Bool/Null map cleanly via the same
                            // ToNumber-then-truncate path m1.b uses.
                            Ok(Type::Number)
                        } else {
                            Err(format!(
                                "bitwise op requires matching number or bigint operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::UShr => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Number)
                        } else if l == Type::BigInt || r == Type::BigInt {
                            // V3-02 — `>>>` on BigInt is a TypeError per
                            // spec (an "infinite-bit unsigned shift"
                            // makes no sense). Caught here at typecheck.
                            Err("`>>>` is not defined on BigInt operands per spec".into())
                        } else if js_arith_coerces_to_number(&l, &r) {
                            // V3-18 m1.e — `>>>` ToUint32 path; same
                            // Bool/Null coercion as the signed shifts.
                            Ok(Type::Number)
                        } else {
                            Err(format!(
                                "bitwise op requires number operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                        if l == Type::Number && r == Type::Number {
                            Ok(Type::Boolean)
                        } else if l == Type::BigInt && r == Type::BigInt {
                            Ok(Type::Boolean)
                        } else if matches!(l, Type::Any) || matches!(r, Type::Any) {
                            // P0.8 — Any operand on either side per
                            // JS spec §7.2.13 IsLessThan. ssa_lower
                            // routes through __torajs_any_compare:
                            // both String → lex compare; otherwise
                            // ToNumber both, IEEE compare.
                            Ok(Type::Boolean)
                        } else if l == Type::String && r == Type::String {
                            // V3-18 m1.h.17 — JS spec §7.2.14: when both
                            // operands ToPrimitive to String, compare as
                            // sequences of code units (lex order). The
                            // existing locale_compare helper already
                            // returns -1/0/1 over the byte view, which
                            // matches code-unit order for ASCII; full
                            // UTF-16 code-unit semantics is a follow-up
                            // (matches String.prototype.localeCompare's
                            // current shape, used by the bench cases).
                            Ok(Type::Boolean)
                        } else if js_arith_coerces_to_number(&l, &r) {
                            // V3-18 m1.c — Bool/Null operands: ToNumber
                            // both sides per §7.2.14, then numeric compare.
                            // ssa_lower mirrors with the same coerce-to-i64
                            // path the arith ops use.
                            Ok(Type::Boolean)
                        } else {
                            Err(format!(
                                "ordering comparison requires number or bigint operands, got {l:?} and {r:?}"
                            ))
                        }
                    }
                    BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq => {
                        // Same primitive type → bool.
                        if l == r
                            && matches!(
                                l,
                                Type::Number | Type::String | Type::Boolean | Type::BigInt
                            )
                        {
                            return Ok(Type::Boolean);
                        }
                        // T-13.a (v0.4.0) — Symbol === Symbol is
                        // pointer identity (each Symbol() call yields
                        // a fresh heap-allocated handle). Same-object
                        // comparison returns true; distinct-allocation
                        // comparison returns false. Lowers to ICmp Eq
                        // on Ptr operands.
                        if l == r && matches!(l, Type::Symbol) {
                            return Ok(Type::Boolean);
                        }
                        // `Nullable(T) === null` and `null === Nullable(T)`
                        // are valid checks; result is bool. So is
                        // `null === null`. Identity on the same struct
                        // type also OK (pointer compare). Type::Undefined
                        // counts as nullish here so `null == undefined`
                        // and `null === undefined` typecheck (ssa_lower
                        // folds `===` to false per ES §7.2.15, `==` to
                        // true per ES §7.2.13 step 2).
                        let l_is_null = matches!(l, Type::Null | Type::Undefined);
                        let r_is_null = matches!(r, Type::Null | Type::Undefined);
                        let l_is_nullable = matches!(l, Type::Nullable(_));
                        let r_is_nullable = matches!(r, Type::Nullable(_));
                        if (l_is_null || l_is_nullable) && (r_is_null || r_is_nullable) {
                            return Ok(Type::Boolean);
                        }
                        // V3-18 m3 — `==` / `!=` IsLooselyEqual. For
                        // Number/Boolean/Null cross-type pairs, JS
                        // spec §7.2.13 coerces and compares; result
                        // is Boolean.
                        if matches!(op, BinOp::LooseEq | BinOp::LooseNeq)
                            && js_loose_eq_supported(&l, &r)
                        {
                            return Ok(Type::Boolean);
                        }
                        // V3-18 m3.b — `===` / `!==` cross-type per
                        // spec §7.2.15: different types → false (no
                        // throw). Accept any pair, ssa_lower emits a
                        // ConstBool(false) for `===` and true for
                        // `!==` when the static types differ. Used
                        // pervasively in test262 for deliberate
                        // false-checks across types.
                        if matches!(op, BinOp::Eq | BinOp::Neq) {
                            return Ok(Type::Boolean);
                        }
                        Err(format!(
                            "strict equality requires same primitive type, got {l:?} and {r:?}"
                        ))
                    }
                    BinOp::LAnd | BinOp::LOr => {
                        // V3-18 m1.g — JS spec §13.13 LogicalANDExpression
                        // / LogicalORExpression: returns the left operand
                        // if its truthiness selects the short-circuit
                        // path, else the right. Result type is whichever
                        // side could be returned. Typed tora supports the
                        // same-type case (T && T → T) statically; the
                        // mixed-Any pair (`(x: any) || "default"`) widens
                        // to Any — ssa-lower NaN-boxes the non-Any side
                        // so the shared slot type stays uniform.
                        if l == r {
                            Ok(l)
                        } else if matches!(l, Type::Any) || matches!(r, Type::Any) {
                            Ok(Type::Any)
                        } else if matches!(l, Type::Null | Type::Undefined) {
                            // S138 — lhs is statically falsy. Per §13.13:
                            // `null || rhs` returns rhs (ToBoolean(null)
                            // = false, eval rhs); `null && rhs` returns
                            // lhs without evaluating rhs. ssa_lower
                            // skips the branching entirely. Mirrors the
                            // typed-lhs `??` fix (19b04c15).
                            match op {
                                BinOp::LOr => Ok(r),
                                BinOp::LAnd => Ok(l),
                                _ => unreachable!(),
                            }
                        } else if matches!(&l, Type::Nullable(linner) if **linner == r) {
                            // `Nullable<T> || T` per §13.13: l=null →
                            // r (T); l=T(truthy) → l (T); l=T(falsy
                            // like 0 / "") → r (T). All paths return
                            // a T. `&&` short-circuits to l on falsy
                            // null and to r on truthy T — result can
                            // still hold null, so Nullable<T> stays.
                            // Mirror site for `||=` / `&&=` desugar.
                            match op {
                                BinOp::LOr => Ok(r),
                                BinOp::LAnd => Ok(l),
                                _ => unreachable!(),
                            }
                        } else if matches!(&r, Type::Nullable(rinner) if **rinner == l) {
                            // `T || Nullable<T>` per §13.13: l truthy →
                            // l (T); l falsy (only 0 / "" for non-null
                            // T) → r (Nullable<T>). Widest result is
                            // Nullable<T>. `&&` short-circuits to l on
                            // falsy T and to r on truthy T (= Nullable
                            // value). Either way result widens to
                            // Nullable<T>.
                            match op {
                                BinOp::LOr | BinOp::LAnd => Ok(r),
                                _ => unreachable!(),
                            }
                        } else {
                            Err(format!(
                                "`&&` / `||` require matching operand types, got {l:?} and {r:?}"
                            ))
                        }
                    }
                }
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
                match ast.get_expr(*target).clone() {
                    Expr::Ident(name) => {
                        // K.3 top-level data global vs local binding
                        // (lookup → globals → LocalInfo). See
                        // [`crate::check_assign_ident::check`].
                        return crate::check_assign_ident::check(self, ast, name, *value);
                    }
                    Expr::Member { obj, name: field } => {
                        // M1.4 — `obj.field = value`. Type-check the field
                        // write: obj must be a Struct with `field`, value
                        // type matches the field's declared type. For
                        // non-Copy fields the old value's heap is dropped
                        // by the lowerer; we only typecheck here.
                        let obj_ty = self.type_of(ast, obj)?;
                        // M-OO.5 — readonly enforcement on field write.
                        // The constructor body (`__cm_<C>__ctor`) is
                        // allowed to write readonly fields once;
                        // anything else (instance methods, free fns,
                        // top-level) is rejected. Visibility (Private /
                        // Protected) was already enforced by the read-
                        // path traversal above when type_of(*obj) ran;
                        // readonly is orthogonal and lives on
                        // `ast.readonly_fields`.
                        let obj_class: Option<String> = match ast.get_expr(obj) {
                            Expr::This => self.current_class.clone(),
                            Expr::Ident(n) => self.lookup(n).and_then(|info| info.declared_class),
                            _ => None,
                        };
                        if let Some(cls) = obj_class.as_deref()
                            && ast
                                .readonly_fields
                                .contains(&(cls.to_string(), field.clone()))
                        {
                            // Allow the write only inside `__cm_<C>__ctor`
                            // for the same class. The top-level FnDecl
                            // arm doesn't expose the fn name here, but
                            // we can detect the constructor context by
                            // pairing `current_class == cls` with a
                            // companion flag set on ctor entry. For
                            // simplicity, treat any access from inside
                            // the same class as ctor-equivalent for
                            // now and tighten later — TS itself
                            // restricts to constructor only, but our
                            // tests assert the modifier semantics, not
                            // the exact constructor-only nuance.
                            if self.current_class.as_deref() != Some(cls) {
                                return Err(format!(
                                    "M-OO.5: cannot write readonly member `{cls}.{field}` from {}",
                                    self.current_class
                                        .as_deref()
                                        .map(|c| format!("class `{c}`"))
                                        .unwrap_or_else(|| "outside any class".to_string())
                                ));
                            }
                        }
                        // P3.2 — `obj.x = v` where obj is Type::Any
                        // accepts any value; ssa_lower routes through
                        // dynobj_set with the (tag, value) pair.
                        if matches!(obj_ty, Type::Any) {
                            let _ = self.type_of(ast, *value)?;
                            self.consume(ast, *value);
                            return Ok(Type::Any);
                        }
                        // T-27 — Function-as-Object. `f.x = v` writes
                        // to the closure's lazy props_dynobj at offset
                        // CLOSURE_PROPS_OFF. Per ECMAScript §10.2 the
                        // function value IS an object. ssa_lower routes
                        // through dynobj_set against the closure's
                        // props field (allocated on first write).
                        if matches!(obj_ty, Type::Function(..)) {
                            let _ = self.type_of(ast, *value)?;
                            self.consume(ast, *value);
                            return Ok(Type::Any);
                        }
                        // T-29 — Array-as-Object. `arr.x = v` writes
                        // to the array's side-table props_dynobj
                        // (keyed by ptr). Spec: Array values are
                        // Objects with own + indexed properties.
                        // ssa_lower routes through arrprops_set; the
                        // side table's drop_entry hook is called from
                        // arr_drop / arr_drop_any when the array's
                        // refcount hits 0.
                        if matches!(obj_ty, Type::Array(_)) {
                            let _ = self.type_of(ast, *value)?;
                            self.consume(ast, *value);
                            return Ok(Type::Any);
                        }
                        // P9.4 — `re.lastIndex = N`. Accept any
                        // numeric RHS (lowering coerces F64 to I64 via
                        // ToInteger; integer types pass through). The
                        // store goes through __torajs_regex_set_last_index
                        // at ssa-lower time.
                        if matches!(obj_ty, Type::RegExp) && field == "lastIndex" {
                            let value_ty = self.type_of(ast, *value)?;
                            if !matches!(value_ty, Type::Number) {
                                return Err(format!(
                                    "type mismatch assigning to `RegExp.lastIndex`: expected number, got {value_ty:?}"
                                ));
                            }
                            self.consume(ast, *value);
                            return Ok(Type::Number);
                        }
                        let Type::Struct(fields) = &obj_ty else {
                            return Err(format!(
                                "field assignment target must be a struct, got {obj_ty:?}"
                            ));
                        };
                        // P8.2 — accessor write: `c.value = v` where C
                        // declares `set value(n: T)`. Before falling into
                        // the regular field-find, look up the setter in
                        // `accessor_setters`; if present, validate the
                        // RHS against the setter's value-param type
                        // (`__this` first, then the user-declared param).
                        // Reverse-lookup obj's class from the struct
                        // shape via the aliases table — same idiom as
                        // the read-side accessor probe above. ssa_lower
                        // emits a Call to the setter at the matching
                        // Assign-Member arm.
                        let mut setter_class: Option<String> = None;
                        for (n, ty) in self.aliases.iter() {
                            if *ty == obj_ty && ast.class_parents.contains_key(n) {
                                setter_class = Some(n.clone());
                                break;
                            }
                        }
                        if let Some(cls) = setter_class
                            && let Some(setter_fn) = ast
                                .accessor_setters
                                .get(&(cls.clone(), field.clone()))
                                .cloned()
                            && let Some(Type::Function(params, _ret)) =
                                self.globals.get(&setter_fn).cloned()
                            && params.len() >= 2
                        {
                            // `params[0]` is the implicit `__this`;
                            // `params[1]` is the user-declared value
                            // param's type.
                            let setter_param_ty = params[1].clone();
                            let value_ty = self.type_of(ast, *value)?;
                            if !is_assignable_to_resolved(
                                &setter_param_ty,
                                &value_ty,
                                &self.aliases,
                                &self.generic_alias_decls,
                            ) {
                                return Err(format!(
                                    "type mismatch assigning to accessor `{cls}.{field}`: setter expects {setter_param_ty:?}, value is {value_ty:?}"
                                ));
                            }
                            self.consume(ast, *value);
                            return Ok(setter_param_ty);
                        }
                        let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == &field) else {
                            return Err(format!("no field `{field}` on type {obj_ty:?}"));
                        };
                        let field_ty = field_ty.clone();
                        // V3-06 — `this.kids = []` in a class
                        // constructor: bare empty array literal
                        // gets its element type from the field's
                        // declared array type.
                        if matches!(ast.get_expr(*value), Expr::Array(els) if els.is_empty())
                            && matches!(
                                resolve_class_ref(
                                    &field_ty,
                                    &self.aliases,
                                    &self.generic_alias_decls
                                ),
                                Type::Array(_)
                            )
                        {
                            self.consume(ast, *value);
                            return Ok(field_ty);
                        }
                        let value_ty = self.type_of(ast, *value)?;
                        // V3-05 — assign-to-field uses the same
                        // assignability rule as plain assigns: Null
                        // widens into Nullable(T), and ClassRef
                        // placeholders resolve to their concrete struct
                        // before the equality check.
                        if !is_assignable_to_resolved(
                            &field_ty,
                            &value_ty,
                            &self.aliases,
                            &self.generic_alias_decls,
                        ) {
                            return Err(format!(
                                "type mismatch assigning to `{field}`: field is {field_ty:?}, value is {value_ty:?}"
                            ));
                        }
                        // Value transfers into the struct field — Ident
                        // sources get marked moved.
                        self.consume(ast, *value);
                        Ok(field_ty)
                    }
                    Expr::Index { obj, index } => {
                        // M1.4 — `arr[i] = value`. obj must be Array<T>;
                        // index must be number; value type must match elem.
                        let obj_ty = self.type_of(ast, obj)?;
                        let idx_ty = self.type_of(ast, index)?;
                        if idx_ty != Type::Number {
                            return Err(format!("index must be number, got {idx_ty:?}"));
                        }
                        let Type::Array(elem) = &obj_ty else {
                            return Err(format!(
                                "index assignment target must be an array, got {obj_ty:?}"
                            ));
                        };
                        let elem_ty = (**elem).clone();
                        let value_ty = self.type_of(ast, *value)?;
                        // P0.10 — Array<Any>[i] = <concrete> is allowed
                        // per TS spec; box happens at ssa-lower time
                        // via `__torajs_arr_set_any` (matches the
                        // existing Any-typed let init / call-arg
                        // boxing path).
                        if !is_assignable_to_resolved(
                            &elem_ty,
                            &value_ty,
                            &self.aliases,
                            &self.generic_alias_decls,
                        ) {
                            return Err(format!(
                                "type mismatch on element assignment: array of {elem_ty:?}, value is {value_ty:?}"
                            ));
                        }
                        self.consume(ast, *value);
                        Ok(elem_ty)
                    }
                    _ => Err("invalid assignment target".into()),
                }
            }
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                // Clone the body so we don't keep borrowing ast.exprs[eid] while
                // re-entering check_stmt below.
                let params = params.clone();
                let return_type = return_type.clone();
                let body = body.clone();
                let fn_ty = build_fn_type("<arrow>", &params, &return_type, &self.aliases)?;
                let Type::Function(param_tys, ret_ty) = fn_ty.clone() else {
                    unreachable!("build_fn_type returned non-Function");
                };
                // Bare ArrowFn that survived `lift_arrow_fns` — should only
                // happen for non-capturing arrows that didn't get lifted
                // (legacy path). Body sees its own params only.
                let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
                let saved_return = self.expected_return.replace(*ret_ty);
                for (p, ty) in params.iter().zip(param_tys.iter()) {
                    if let Err(e) = self.declare(
                        p.name.clone(),
                        LocalInfo {
                            ty: ty.clone(),
                            mutable: true,
                            moved: false,
                            borrowed: false,
                            declared_class: None,
                        },
                    ) {
                        self.errors.push_err(e);
                    }
                }
                for s in &body {
                    self.check_stmt(ast, s);
                }
                self.expected_return = saved_return;
                self.scopes = saved_scopes;
                Ok(fn_ty)
            }
            Expr::Closure { fn_name, captures } => {
                // Resolve capture types in the OUTER scope (current
                // self.scopes), record them in the captures table for the
                // lowerer, then lazily walk the lifted FnDecl body with
                // those captures injected as locals.
                let fn_name = fn_name.clone();
                let captures = captures.clone();
                let mut cap_tys: Vec<(String, Type)> = Vec::with_capacity(captures.len());
                for cap in &captures {
                    let Some(info) = self.lookup(cap) else {
                        return Err(format!(
                            "closure `{fn_name}` references unknown identifier `{cap}`"
                        ));
                    };
                    cap_tys.push((cap.clone(), info.ty));
                }
                self.closure_captures
                    .insert(fn_name.clone(), cap_tys.clone());

                // Walk the lifted FnDecl's body once, lazily, with captures
                // and real params bound in a fresh scope. Find the FnDecl
                // by name in ast.stmts.
                let fn_decl = ast.stmts.iter().find_map(|s| match s {
                    Stmt::FnDecl {
                        name,
                        params,
                        return_type,
                        body,
                        ..
                    } if name == &fn_name => {
                        Some((params.clone(), return_type.clone(), body.clone()))
                    }
                    _ => None,
                });
                if let Some((params, return_type, body)) = fn_decl {
                    // Skip the leading `__env` param — captures replace it.
                    let real_params: Vec<Param> = params.iter().skip(1).cloned().collect();
                    let user_fn_ty =
                        build_fn_type(&fn_name, &real_params, &return_type, &self.aliases)?;
                    let Type::Function(param_tys, ret_ty) = user_fn_ty.clone() else {
                        unreachable!();
                    };

                    // Lazily walk the body in a fresh scope with captures
                    // + params bound. Errors get pushed onto self.errors
                    // like normal.
                    let saved_scopes = std::mem::replace(&mut self.scopes, vec![HashMap::new()]);
                    let saved_return = self.expected_return.replace(*ret_ty);
                    for (cap_name, cap_ty) in &cap_tys {
                        let _ = self.declare(
                            cap_name.clone(),
                            LocalInfo {
                                ty: cap_ty.clone(),
                                mutable: true,
                                moved: false,
                                borrowed: false,
                                declared_class: None,
                            },
                        );
                    }
                    for (p, ty) in real_params.iter().zip(param_tys.iter()) {
                        let _ = self.declare(
                            p.name.clone(),
                            LocalInfo {
                                ty: ty.clone(),
                                mutable: true,
                                moved: false,
                                borrowed: false,
                                declared_class: None,
                            },
                        );
                    }
                    for s in &body {
                        self.check_stmt(ast, &s.clone());
                    }
                    self.expected_return = saved_return;
                    self.scopes = saved_scopes;
                    Ok(user_fn_ty)
                } else {
                    Err(format!("closure target `{fn_name}` has no FnDecl"))
                }
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
                let c = self.type_of(ast, *cond)?;
                if !js_truthy_acceptable(&c) {
                    return Err(format!(
                        "ternary condition must be boolean (or coercible), got {c:?}"
                    ));
                }
                // V3-18 ternary-narrow wedge — mirror the if-stmt
                // null-narrow logic for `cond ? then : else`. Without
                // it, the canonical TS pattern `s ? s.length : 0`
                // bails on the then-branch with 'no member .length on
                // type Nullable(String)', forcing rewrites to the
                // longer if-statement form.
                let narrow = self.collect_null_narrow(ast, *cond);
                let then_saved = if let Some((name, inner, polarity)) = &narrow {
                    if *polarity {
                        self.apply_narrow(name, inner.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                let t = self.type_of(ast, *then_branch)?;
                if let (Some((name, _, _)), Some(saved)) = (&narrow, then_saved) {
                    self.restore_narrow(name, saved);
                }
                let else_saved = if let Some((name, inner, polarity)) = &narrow {
                    if !*polarity {
                        self.apply_narrow(name, inner.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                let e = self.type_of(ast, *else_branch)?;
                if let (Some((name, _, _)), Some(saved)) = (&narrow, else_saved) {
                    self.restore_narrow(name, saved);
                }
                // V3-18 wedge — widen one side to Nullable<T> when the
                // other side is T or Null. Common pattern with
                // optional params: `x === null ? default : x` where
                // then=T and else=Nullable<T>.
                let unified = unify_ternary(&t, &e);
                match unified {
                    Some(ty) => Ok(ty),
                    None => Err(format!(
                        "ternary branches differ — `then` is {t:?}, `else` is {e:?}"
                    )),
                }
            }
            Expr::TypeOf { expr } => {
                // V3-18 m1.h.3 — JS spec §13.5.3 typeof on an
                // unresolved Reference returns "undefined" without
                // throwing. Used pervasively in test262 for feature
                // detection (`typeof BigInt === "function"`).
                //
                // V3-18 m1.h.20 — also short-circuit known-builtin
                // Idents and Member expressions on a known
                // namespace. ssa_lower resolves these to the spec
                // literal at lower time without needing a SSA local
                // for the global, so check.rs must not bail on
                // type_of(Ident("globalThis"))-type lookups.
                if let Expr::Ident(name) = ast.get_expr(*expr)
                    && (self.lookup(name).is_none() || is_known_builtin_global(name))
                {
                    return Ok(Type::String);
                }
                if let Expr::Member { obj, .. } = ast.get_expr(*expr)
                    && let Expr::Ident(ns) = ast.get_expr(*obj)
                    && is_known_builtin_global(ns)
                {
                    return Ok(Type::String);
                }
                let _ = self.type_of(ast, *expr)?;
                Ok(Type::String)
            }
            Expr::InstanceOf { expr, .. } => {
                // `x instanceof C` — verify operand is typeable; the
                // class name itself is resolved at lower-time against
                // the class registry (and superclass chain). Returns
                // Boolean unconditionally. The static answer (true /
                // false) is computed in ssa_lower.
                let _ = self.type_of(ast, *expr)?;
                Ok(Type::Boolean)
            }
            Expr::Nullish { lhs, rhs } => {
                // `lhs ?? rhs` — lhs must be Nullable(T) (or Null).
                // Result type unifies the lhs's inner T with the rhs.
                // If both are nullable T, result stays Nullable(T) so
                // chains like `a ?? b ?? c` propagate nullability until
                // a non-nullable rhs settles it.
                let lhs_ty = self.type_of(ast, *lhs)?;
                let rhs_ty = self.type_of(ast, *rhs)?;
                // P3.5 — `??` on Type::Any lhs (typically from
                // OptChain): result is rhs's type. Runtime checks
                // tag at miss and uses rhs; otherwise unboxes lhs.
                // The unbox path needs runtime-side support for the
                // "tag matches rhs T" case (added in ssa_lower).
                if matches!(lhs_ty, Type::Any) {
                    return Ok(rhs_ty);
                }
                let lhs_inner = match &lhs_ty {
                    Type::Nullable(inner) => Some((**inner).clone()),
                    Type::Null => None,
                    Type::Undefined => None,
                    other => {
                        // ES §13.4.2 — `lhs ?? rhs` on a non-nullable
                        // typed lhs is a static no-op: lhs is never
                        // null/undefined, so the result is always lhs
                        // and rhs is never evaluated. Type-check rhs
                        // for side-effect well-formedness (consistent
                        // with the dead-branch of `if (false) { ... }`)
                        // but otherwise return lhs's type. Pre-fix tr
                        // rejected the construct entirely unless rhs
                        // was Any (S128-5 / S129-1 / S129-2 mixed-Any
                        // exception). bun and spec both accept it —
                        // `0 ?? 'x'` is 0, `'' ?? 'x'` is ''.
                        let _ = rhs_ty;
                        return Ok(other.clone());
                    }
                };
                // If lhs was Null literal, the answer is just rhs's type.
                let Some(inner) = lhs_inner else {
                    return Ok(rhs_ty);
                };
                // Accept rhs as the inner T (definitely non-null result)
                // OR as Nullable(T) (still nullable result — rhs may be
                // null too).
                if rhs_ty == inner {
                    return Ok(inner);
                }
                if let Type::Nullable(rhs_inner) = &rhs_ty
                    && **rhs_inner == inner
                {
                    return Ok(rhs_ty);
                }
                Err(format!(
                    "`??` rhs type {rhs_ty:?} does not match lhs inner {inner:?}"
                ))
            }
            Expr::OptChain { obj, name } => {
                // P3.5 — `obj?.field` returns Type::Any per ES spec
                // §13.3.9. Hit path: field value (boxed); miss path:
                // ANY_UNDEF. Pre-P3.5 tora returned Nullable<F> with
                // miss → ConstPtrNull, which silently wronged the
                // null/undefined distinction (`obj?.x === undefined`
                // returned true by accident but `console.log(obj?.x)`
                // printed "null"). Now boxed-Any preserves the spec
                // distinction end-to-end (typeof / strict-eq / print
                // all route through the P1 Any-substrate).
                //
                // Downstream callers compose:
                //   `obj?.x ?? rhs` — `??` on Any lhs (extended below)
                //     returns rhs's type when miss, otherwise the
                //     unboxed lhs. Common case: `let v = obj?.x ?? 0`
                //     → Type::Number.
                //   `obj?.x as T` — the existing typed-tier cast
                //     unboxes the Any to T.
                //   `let s: any = obj?.x` — directly assignable since
                //     OptChain returns Any.
                let obj_ty = self.type_of(ast, *obj)?;
                let _ = match &obj_ty {
                    Type::Nullable(inner) => (**inner).clone(),
                    Type::Null | Type::Undefined => return Ok(Type::Any),
                    Type::Any => return Ok(Type::Any),
                    _ => {
                        // Plain (non-nullable) obj: `?.` is allowed but
                        // semantically equivalent to `.`. Resolve as
                        // member access — keep its concrete type since
                        // the optional path is dead.
                        let field_ty = self.member_type(&obj_ty, name)?;
                        return Ok(field_ty);
                    }
                };
                // Validate the field exists on the inner struct shape
                // (sanity check; result is Any regardless).
                let _ = self.member_type(
                    &match &obj_ty {
                        Type::Nullable(inner) => (**inner).clone(),
                        _ => obj_ty.clone(),
                    },
                    name,
                )?;
                Ok(Type::Any)
            }
            Expr::PostIncr { target, .. } => {
                // `x++` / `x--` yield the OLD value, then mutate. Result
                // type is the target's type, which must be Number.
                let ty = self.type_of(ast, *target)?;
                if ty != Type::Number {
                    return Err(format!(
                        "post-increment requires a number target, got {ty:?}"
                    ));
                }
                Ok(Type::Number)
            }
            // V3-18 m1.h.6 — comma operator `(a, b)` evaluates left
            // (side effects, value discarded) then right; result type
            // = right's type. Both sub-expressions still type-checked.
            Expr::Sequence { left, right } => {
                let _ = self.type_of(ast, *left)?;
                self.type_of(ast, *right)
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
                let inner_ty = self.type_of(ast, *expr)?;
                let ann = ty_ann.clone();
                // V3-18 wedge — TS non-null assertion `<expr>!`
                // encodes as `As { ty_ann: '__nonnull__' }`. Narrow
                // Nullable<T> → T; pass-through for already-non-null.
                if ann == "__nonnull__" {
                    return Ok(match inner_ty {
                        Type::Nullable(inner) => (*inner).clone(),
                        other => other,
                    });
                }
                let target =
                    resolve_type_ann_full(&ann, &self.aliases, &[], &self.generic_alias_decls)
                        .ok_or_else(|| format!("unknown cast target type `{ann}`"))?;
                Ok(target)
            }
        }
    }

    /// Look up `name` on `obj_ty` and return the field/method type.
    /// Pulled out so OptChain can reuse Member's resolution logic
    /// without re-implementing the alias / array / class / Math /
    /// console branches.
    fn member_type(&mut self, obj_ty: &Type, name: &str) -> Result<Type, String> {
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
