//! What the checker knows about ONE scope binding.
//!
//! Pulled out of [`crate::check`] verbatim (chunk-573 of the check.rs
//! god-file decomp) — the file was at the 500-line hard limit and this
//! is the seam its own contents draw: everything else in check.rs
//! describes the CHECKER, this describes a single entry in its scope
//! frames. Re-exported from `crate::check` so the canonical
//! `crate::check::LocalInfo` import path keeps working.

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
}
