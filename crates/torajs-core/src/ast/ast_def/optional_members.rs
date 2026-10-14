//! The members a type or class declaration marks optional (`m?: T`,
//! `m?(): T`), keyed `(type, member)`.
//!
//! An optional member's annotation is respelled `__nullable(T)`, which
//! alone also spells a required `T | null`, so the declaration's mark
//! is kept here. [`OptionalMembers::plain`] narrows it to the members
//! whose written type spells neither `null` nor `undefined` (the parser
//! added the wrapper itself): such a `number` member keeps an f64 slot
//! with the undefined sentinel in it, while `m?: T | null` boxes, as it
//! may hold `null`.

use std::collections::HashSet;

use super::super::PropKey;

#[derive(Debug, Clone, Default)]
pub struct OptionalMembers {
    /// Every optional member of an interface / object type declaration.
    pub all: HashSet<(String, PropKey)>,
    /// Every optional member of a type or class declaration whose
    /// written type spells neither `null` nor `undefined`.
    pub plain: HashSet<(String, PropKey)>,
}
