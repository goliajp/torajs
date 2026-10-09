//! Whether two slot rows' annotations name the same type.
//!
//! The slot joins compare rows by annotation, and a textual mismatch
//! widens the whole slot to `any`. Two spellings of one type
//! (`number[]` / `Array<number>`, an alias and its target) are not a
//! disagreement, so before widening a join asks whether the two
//! annotations resolve to the same type. Only types with no nominal
//! part count: two classes of one shape resolve alike but are not
//! interchangeable.

use std::cell::OnceCell;

use crate::ast::Ast;
use crate::check::Type;
use crate::check_pipeline::{TypeAliasTables, collect_type_alias_tables};

pub(crate) struct AnnEq<'a> {
    ast: &'a Ast,
    tables: OnceCell<TypeAliasTables>,
}

impl<'a> AnnEq<'a> {
    pub(crate) fn new(ast: &'a Ast) -> Self {
        AnnEq {
            ast,
            tables: OnceCell::new(),
        }
    }

    /// Equal spellings, or spellings that resolve to one structural
    /// type; an annotation that does not resolve (a type parameter, a
    /// class) only matches its own spelling.
    pub(crate) fn same(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        let t = self
            .tables
            .get_or_init(|| collect_type_alias_tables(self.ast));
        let resolve = |s: &str| {
            crate::check_type_ann::resolve_type_ann_full(s, &t.aliases, &[], &t.generic_alias_decls)
        };
        match (resolve(a), resolve(b)) {
            (Some(x), Some(y)) => x == y && structural(&x),
            _ => false,
        }
    }

    /// Whether a value of `from` fits a slot of `to` by the checker's
    /// assignability rules.
    pub(crate) fn fits(&self, to: &str, from: &str) -> bool {
        let t = self
            .tables
            .get_or_init(|| collect_type_alias_tables(self.ast));
        let resolve = |s: &str| {
            crate::check_type_ann::resolve_type_ann_full(s, &t.aliases, &[], &t.generic_alias_decls)
        };
        let (Some(to), Some(from)) = (resolve(to), resolve(from)) else {
            return false;
        };
        crate::check_assignable::is_assignable_to_resolved(
            &to,
            &from,
            &t.class_structs,
            &t.aliases,
            &t.generic_alias_decls,
        )
    }

    /// Whether `wide` is `narrow` or nullish, with `narrow` a type held
    /// by pointer (an object, a string, an array): a slot of either
    /// stores a value of `narrow` alike, and a nullish one as the
    /// pointer slot's own null or undefined.
    pub(crate) fn nullable_of(&self, wide: &str, narrow: &str) -> bool {
        let t = self
            .tables
            .get_or_init(|| collect_type_alias_tables(self.ast));
        let resolve = |s: &str| {
            crate::check_type_ann::resolve_type_ann_full(s, &t.aliases, &[], &t.generic_alias_decls)
        };
        match (resolve(wide), resolve(narrow)) {
            (Some(Type::Nullable(w)), Some(n)) => {
                *w == n
                    && matches!(
                        n,
                        Type::ClassRef(_) | Type::Struct(_) | Type::String | Type::Array(_)
                    )
            }
            _ => false,
        }
    }
}

fn structural(t: &Type) -> bool {
    match t {
        Type::Number
        | Type::String
        | Type::Boolean
        | Type::Void
        | Type::BigInt
        | Type::Any
        | Type::Null
        | Type::Undefined
        | Type::Symbol => true,
        Type::Array(x) | Type::Nullable(x) | Type::Promise(x) => structural(x),
        Type::Map(k, v) => structural(k) && structural(v),
        _ => false,
    }
}
