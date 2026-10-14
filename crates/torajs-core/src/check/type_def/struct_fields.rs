//! The field list of a [`Type::Struct`], with the optional bit each
//! field carries.
//!
//! An optional property (`a?: T`, TS §3.9.2) and a required one whose
//! type admits null or undefined (`a: T | null`) have the same field
//! type: the annotation folds both nullish members into
//! `__nullable(T)`. What tells them apart is the property's own
//! optional flag — only an optional property may be left out of an
//! object literal (a required one missing is TS2741). The flag rides
//! beside the fields, so every spelling that reaches a struct type
//! (an object type literal, an interface or type alias, a generic
//! instance) can say which members are optional.
//!
//! The flag is no part of the type's identity: two struct types
//! compare equal on their fields alone, as they did before the flag
//! existed, so assignability and unification are unchanged. A
//! transform that rebuilds the list from an iterator drops the flags,
//! which only ever refuses a literal that leaves a member out — the
//! loud side.

use core::fmt;
use core::ops::{Deref, DerefMut};

use super::Type;
use crate::ast::{Ast, PropKey};

#[derive(Clone, Default)]
pub struct StructFields {
    fields: Vec<(PropKey, Type)>,
    optional: Vec<PropKey>,
}

impl StructFields {
    /// The fields of the declaration `owner` (an interface or object
    /// type alias), its optional members flagged.
    pub(crate) fn declared(ast: &Ast, owner: &str, fields: Vec<(PropKey, Type)>) -> StructFields {
        let optional = StructFields::declared_optional(ast, owner, &fields);
        StructFields::from(fields).with_optional(optional)
    }

    /// The members of the declaration `owner` it marks optional.
    pub(crate) fn declared_optional<T>(
        ast: &Ast,
        owner: &str,
        fields: &[(PropKey, T)],
    ) -> Vec<PropKey> {
        fields
            .iter()
            .filter(|(n, _)| {
                ast.optional_members
                    .all
                    .contains(&(owner.to_string(), n.clone()))
            })
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// The same fields, with `names` marked optional.
    pub fn with_optional(mut self, names: impl IntoIterator<Item = PropKey>) -> StructFields {
        for n in names {
            if !self.optional.contains(&n) {
                self.optional.push(n);
            }
        }
        self
    }

    /// Whether the member `name` was declared optional.
    pub fn is_optional(&self, name: &PropKey) -> bool {
        self.optional.contains(name)
    }

    /// The members declared optional.
    pub fn optional(&self) -> &[PropKey] {
        &self.optional
    }

    /// Each field's type through `f`, the flags kept.
    pub fn map_types(self, mut f: impl FnMut(Type) -> Type) -> StructFields {
        let fields = self.fields.into_iter().map(|(k, t)| (k, f(t))).collect();
        StructFields {
            fields,
            optional: self.optional,
        }
    }

    /// [`Self::map_types`] over a borrowed list.
    pub fn map_types_ref(&self, mut f: impl FnMut(&Type) -> Type) -> StructFields {
        let fields = self.fields.iter().map(|(k, t)| (k.clone(), f(t))).collect();
        StructFields {
            fields,
            optional: self.optional.clone(),
        }
    }

    /// The field list, the flags dropped.
    pub fn into_vec(self) -> Vec<(PropKey, Type)> {
        self.fields
    }
}

impl Type {
    /// A struct type of `fields`, none of them optional.
    pub fn struct_of(fields: Vec<(PropKey, Type)>) -> Type {
        Type::Struct(StructFields::from(fields))
    }

    /// Whether the struct `from` declares optional a member the struct
    /// `to` requires: a value of `from` may lack it, so it does not fit
    /// `to` (TS2322), however equal the field types are. The flags are
    /// no part of struct equality, so the question goes down through
    /// the members both sides name and the element types around them.
    pub fn optional_into_required(to: &Type, from: &Type) -> bool {
        match (to, from) {
            (Type::Struct(t), Type::Struct(f)) => t.iter().any(|(k, tt)| {
                f.iter().any(|(fk, ft)| {
                    fk == k
                        && ((f.is_optional(k) && !t.is_optional(k))
                            || Type::optional_into_required(tt, ft))
                })
            }),
            (Type::Array(t), Type::Array(f))
            | (Type::Nullable(t), Type::Nullable(f))
            | (Type::Promise(t), Type::Promise(f)) => Type::optional_into_required(t, f),
            (Type::Nullable(t), f) => Type::optional_into_required(t, f),
            _ => false,
        }
    }
}

impl PartialEq for StructFields {
    fn eq(&self, other: &StructFields) -> bool {
        self.fields == other.fields
    }
}

impl fmt::Debug for StructFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fields.fmt(f)
    }
}

impl Deref for StructFields {
    type Target = Vec<(PropKey, Type)>;
    fn deref(&self) -> &Vec<(PropKey, Type)> {
        &self.fields
    }
}

impl DerefMut for StructFields {
    fn deref_mut(&mut self) -> &mut Vec<(PropKey, Type)> {
        &mut self.fields
    }
}

impl From<Vec<(PropKey, Type)>> for StructFields {
    fn from(fields: Vec<(PropKey, Type)>) -> StructFields {
        StructFields {
            fields,
            optional: Vec::new(),
        }
    }
}

impl FromIterator<(PropKey, Type)> for StructFields {
    fn from_iter<I: IntoIterator<Item = (PropKey, Type)>>(iter: I) -> StructFields {
        StructFields::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl IntoIterator for StructFields {
    type Item = (PropKey, Type);
    type IntoIter = std::vec::IntoIter<(PropKey, Type)>;
    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter()
    }
}

impl<'a> IntoIterator for &'a StructFields {
    type Item = &'a (PropKey, Type);
    type IntoIter = core::slice::Iter<'a, (PropKey, Type)>;
    fn into_iter(self) -> Self::IntoIter {
        self.fields.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_optional_member_does_not_fit_a_required_one() {
        let fields = vec![
            (PropKey::from("a"), Type::Number),
            (PropKey::from("b"), Type::Number),
        ];
        let a = [PropKey::from("a")];
        let opt = Type::Struct(StructFields::from(fields.clone()).with_optional(a));
        let req = Type::struct_of(fields);
        assert!(Type::optional_into_required(&req, &opt));
        assert!(!Type::optional_into_required(&opt, &req));
        assert!(!Type::optional_into_required(&opt, &opt));
        assert!(!Type::optional_into_required(&req, &req));
        let nest = |t: &Type| Type::struct_of(vec![(PropKey::from("x"), t.clone())]);
        assert!(Type::optional_into_required(&nest(&req), &nest(&opt)));
        assert!(!Type::optional_into_required(&nest(&opt), &nest(&req)));
        let arr = |t: &Type| Type::Array(Box::new(t.clone()));
        assert!(Type::optional_into_required(&arr(&req), &arr(&opt)));
        let nullable = Type::Nullable(Box::new(req.clone()));
        assert!(Type::optional_into_required(&nest(&nullable), &nest(&opt)));
    }
}
