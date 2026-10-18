//! The interned `Array<T>` element types ([`ArrLayouts`]): `ArrId` is
//! the index, and two arrays of the same element type share one.
//!
//! An array whose elements are instances of a value class (pass 0.5's
//! value-class census) holds their fields in place: its element type
//! interns as [`Type::Inline`] of the class, never as [`Type::Obj`],
//! so every array of the class in the program shares that one layout.

use std::collections::HashSet;

use super::{ArrId, StructId, Type};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArrLayouts {
    elems: Vec<Type>,
    /// The value classes whose arrays hold their fields in place.
    /// An element type interned before this set is filled (a field's
    /// array type pass 0.5 resolves with its class layouts, such as a
    /// container's `toks: Token[]`) still reads `Obj`;
    /// [`Self::hold_in_place`] rewrites those entries in place, so that
    /// every array of the class keeps sharing one id.
    value_sids: HashSet<StructId>,
}

impl ArrLayouts {
    /// The id of the array of `elem`, interning it when new.
    pub fn intern(&mut self, elem: Type) -> ArrId {
        let elem = match elem {
            Type::Obj(sid) if self.value_sids.contains(&sid) => Type::Inline(sid),
            e => e,
        };
        if let Some(i) = self.elems.iter().position(|e| *e == elem) {
            return ArrId(i as u32);
        }
        self.elems.push(elem);
        ArrId(self.elems.len() as u32 - 1)
    }

    pub fn len(&self) -> usize {
        self.elems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elems.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&Type> {
        self.elems.get(i)
    }

    /// Whether some class of the program has its arrays held in place.
    pub fn has_value_classes(&self) -> bool {
        !self.value_sids.is_empty()
    }

    /// Whether the arrays of class `sid` hold its instances in place.
    pub fn holds_in_place(&self, sid: StructId) -> bool {
        self.value_sids.contains(&sid)
    }

    /// Hold the arrays of `sids` in place from now on, and rewrite the
    /// arrays of them interned so far, keeping their ids.
    pub(crate) fn hold_in_place(&mut self, sids: impl IntoIterator<Item = StructId>) {
        self.value_sids.extend(sids);
        for e in &mut self.elems {
            if let Type::Obj(sid) = *e
                && self.value_sids.contains(&sid)
            {
                *e = Type::Inline(sid);
            }
        }
    }

    /// An empty table whose arrays of `sids` hold their fields in
    /// place, for tests that lower an inline array by hand.
    #[cfg(test)]
    pub(crate) fn with_value_classes(sids: &[StructId]) -> Self {
        let mut t = ArrLayouts::default();
        t.hold_in_place(sids.iter().copied());
        t
    }
}

impl std::ops::Index<usize> for ArrLayouts {
    type Output = Type;

    fn index(&self, i: usize) -> &Type {
        &self.elems[i]
    }
}

/// A table of exactly these element types, in order, with no value
/// class (hand-built modules).
impl From<Vec<Type>> for ArrLayouts {
    fn from(elems: Vec<Type>) -> Self {
        ArrLayouts {
            elems,
            value_sids: HashSet::new(),
        }
    }
}
