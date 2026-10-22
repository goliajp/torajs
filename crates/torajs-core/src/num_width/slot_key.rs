//! The storage slot identity every number-width table is keyed by, as
//! spelled; the analysis works on its interned form (`slot_keys.rs`).

use crate::ast::PropKey;

/// Identity of a number-typed storage slot, module-wide.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SlotKey {
    /// Top-level `let` / `const` binding (whether it later promotes to
    /// a data global or localizes into main — both consumers key here).
    Global(String),
    /// Fn-local `let` binding: (fn name, var name). Block-scoped
    /// same-name lets within one fn share a key — merging their
    /// poison is conservative (F64-ward), never wrong.
    Local(String, String),
    /// Fn parameter: (fn name, param name).
    Param(String, String),
    /// Fn return slot.
    Ret(String),
    /// W4 — nominal aggregation point for a class: every instance of
    /// `class C` shares one struct layout at lowering, so field-width
    /// decisions join over all instances. `__new_C`'s ret and every
    /// `__cm_C__*` method's `__this` param union into this key.
    Class(String),
    /// W4 — a name a lifted body reads that no binding resolves
    /// (`captures.rs`): every same-named slot unions through it.
    Captured(String),
    /// W4 — anonymous container origin (array / object literal, or a
    /// transform-method result), keyed by the originating ExprId.
    Anon(u32),
    /// W4 — element-width point of the container alias class whose
    /// representative is the inner key. `number[]` elems narrow to I64
    /// only when every write reaching any alias of the array is
    /// provably integral.
    Elem(Box<SlotKey>),
    /// W4 — field-width point of a struct / class / inline-object
    /// alias class: (container class representative, field name).
    Field(Box<SlotKey>, PropKey),
    /// RFC 20260726-array-elem-width — the pending-throw value slot.
    ///
    /// There is one of it in the runtime, and it holds raw 8 bytes that
    /// a `throw` writes and a `catch` binding reads back, so both ends
    /// have to agree on how to read those bits — the same reason a
    /// promise's `value` point exists. Nothing joined them before, so
    /// `throw xs[i]` on a widened array wrote f64 bits that
    /// `catch (e: number)` read as an integer.
    ///
    /// Being a single point means every numeric throw site in a program
    /// shares one class. That is the conservative direction: a class
    /// merges F64-ward, and a JS number IS an f64, so the worst case is
    /// a slot that stays wider than it had to.
    Thrown,
}
