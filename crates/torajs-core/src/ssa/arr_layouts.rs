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
