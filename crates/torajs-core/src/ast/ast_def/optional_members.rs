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
    /// Some optional member neither set names: an inline object type's
    /// (`p: { a?: T }`), or a class field whose written type spells
    /// `null` / `undefined`.
    pub unnamed: bool,
}

impl OptionalMembers {
    /// Whether the program writes no optional member (`m?`) at all.
    pub fn none(&self) -> bool {
        self.all.is_empty() && self.plain.is_empty() && !self.unnamed
    }

    /// Whether the declaration `owner` marks `key` optional ([`Self::all`]).
    /// The key is built only when the set has a member.
    pub fn declared(&self, owner: &str, key: &PropKey) -> bool {
        !self.all.is_empty() && self.all.contains(&(owner.to_string(), key.clone()))
    }

    /// Whether `key` is a plain optional member of `owner`
    /// ([`Self::plain`]).
    pub fn plain_at(&self, owner: &str, key: &PropKey) -> bool {
        !self.plain.is_empty() && self.plain.contains(&(owner.to_string(), key.clone()))
    }
}
