//! What the checker knows about ONE scope binding.
//!
//! Pulled out of [`crate::check`] verbatim (chunk-573 of the check.rs
//! god-file decomp) — the file was at the 500-line hard limit and this
//! is the seam its own contents draw: everything else in check.rs
//! describes the CHECKER, this describes a single entry in its scope
//! frames. Re-exported from `crate::check` so the canonical
//! `crate::check::LocalInfo` import path keeps working.

use crate::ast::ExprId;
use crate::check::Type;

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
    /// This binding was initialized from a builtin MEMBER VALUE read
    /// — a prototype method (`const m = s.slice`, RFC
    /// 20260725-str-method-value-reify) or a namespace static
    /// (`const f = String.fromCharCode`).
    ///
    /// Every call through such a binding routes to the boxed dual
    /// entry, where the kernel runs the spec's own steps: arity per
    /// the meta row, and ToNumber / ToString on the arguments. So the
    /// member table's signature describes the answer but must not be
    /// enforced on the way in — neither its fixed arity for
    /// `.call` / `.apply` / `.bind`, nor its parameter types for a
    /// plain call. `String.fromCharCode("65")` is "A" (§22.1.2.1
    /// step 2 is ToUint16, a coercion, not a shape test), and it has
    /// to stay "A" when the function arrives through a binding.
    pub(crate) builtin_mv: bool,
    /// This binding holds a `re.exec(s)` result the exec-record lane
    /// keeps as spans (`crate::check_exec_record`): `index` reads a
    /// number and `input` a string, because nothing can reach the
    /// binding to make them anything else. Every other member reads
    /// as on any `string[] | null`.
    pub(crate) exec_result: bool,
    /// This binding is a fn-typed slot the face census recorded
    /// (`Ast::fnsig_mismatch_bindings` / `Ast::argv_boxed_params`),
    /// so its calls ride the boxed entry
    /// (`check_type_of_call::fn_slot_ret`).
    pub(crate) fn_slot: Option<FnSlot>,
    /// What the union rules know about this binding, when it is typed
    /// as a union (see `check/union_narrow.rs`).
    pub(crate) union: UnionBinding,
    /// An object-typed binding that holds an `any` value
    /// (`crate::check_stmt_let_decl::held_any`).
    pub(crate) held_any: bool,
    /// The annotation the declaration wrote (`let x: T`, `p: T`), for
    /// a binding the program typed itself; `None` for one the compiler
    /// typed.
    pub(crate) ann: Option<String>,
    /// Which declaration a write through the name reaches: a `let` /
    /// `const` / `var` by its initializer (a `for-of` head by its
    /// element read), or a fn-typed parameter by its function.
    pub(crate) decl: Option<FnSlot>,
    /// The declared `T | null` behind a narrow that a later write may
    /// retire (`crate::check_assigns_to`): set while a guarded region
    /// that writes the binding is walked, and from an `if` whose
    /// branches both fill the binding on. A write is checked against
    /// it, and a possibly-null one puts the binding back to it.
    pub(crate) flow_declared: Option<Type>,
    /// The `T | null` a null narrow first replaced. A closure reads the
    /// binding whenever it is called, so it captures this type when a
    /// write could undo the narrow (`check/capture_narrow.rs`).
    pub(crate) null_declared: Option<Type>,
    /// The values of the possibly-null stores into the binding walked
    /// so far.
    pub(crate) null_stores: Vec<ExprId>,
    /// A possibly-null store into the binding can run at any time: it
    /// sits in a closure, or the binding is top-level and a function
    /// body writes it.
    pub(crate) nulled_anytime: bool,
    /// A closure reads the binding at its non-null narrow, so a
    /// possibly-null store into it is refused from here on.
    pub(crate) pinned: bool,
    /// The entry is a closure's view of a binding declared outside it:
    /// a store into it in the body runs again on every later call.
    pub(crate) captured: bool,
}

impl LocalInfo {
    /// A mutable binding of type `ty` with every flag clear.
    pub(crate) fn plain(ty: Type) -> LocalInfo {
        LocalInfo {
            ty,
            mutable: true,
            moved: false,
            borrowed: false,
            declared_class: None,
            builtin_mv: false,
            exec_result: false,
            fn_slot: None,
            union: UnionBinding::default(),
            held_any: false,
            ann: None,
            decl: None,
            flow_declared: None,
            null_declared: None,
            null_stores: Vec::new(),
            nulled_anytime: false,
            pinned: false,
            captured: false,
        }
    }
}

/// The per-binding half of the union rules.
#[derive(Debug, Clone, Default)]
pub(crate) struct UnionBinding {
    /// The members the binding was declared with, recorded the first
    /// time a narrow replaces its type; `None` while it still holds
    /// its declared union.
    pub(crate) declared: Option<Vec<Type>>,
    /// Under a guard the checker does not narrow, or assigned to: a
    /// member read needs the name on one declared member only.
    pub(crate) loose: bool,
    /// The literal type of an unannotated `const` initialized with a
    /// literal (TS §3.2.10: a `const` keeps the literal, a `let`
    /// widens). Its reads type as this; the binding's type stays the
    /// base.
    pub(crate) literal: Option<Type>,
}

/// Which declaration a census slot binding is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FnSlot {
    /// a `let` / `const` / `var`, by its initializer
    Let(crate::ast::ExprId),
    /// a parameter of the named function (reached directly, or as a
    /// capture of a closure inside it)
    Param(String),
}

impl FnSlot {
    /// The declaration a parameter of `owner` typed `ty` is: only a
    /// fn-typed one can be handed a value of another face.
    pub(crate) fn param(owner: &str, ty: &Type) -> Option<FnSlot> {
        matches!(ty, Type::Function(..)).then(|| FnSlot::Param(owner.to_string()))
    }
}
