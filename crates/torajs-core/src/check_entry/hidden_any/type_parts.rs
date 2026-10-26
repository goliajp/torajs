//! The types a type is built from, one level down: an array's element,
//! an object type's members, a function's parameters and result, a
//! map's or a dictionary's key and value, a promise's value, a union's
//! members. Every walk of the rounds over a type's parts goes through
//! the one list of container types here, so no walk misses one another
//! walk reaches.

use crate::check::Type;

/// What a part is to the type it is part of.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    /// a function's parameter: a value of the function type is handed
    /// values of it, never hands one on
    Param,
    /// every other part, which a value of the type hands on
    Held,
}

/// The one list of container types: `$visit` is run on each part of
/// `$t`, bound as `$c` by a reference of type `$r` (`&Type` or
/// `&mut Type`), with its [`Part`] bound as `$k`; `$iter` names the
/// slice iterator (`iter` or `iter_mut`).
macro_rules! each_part {
    ($t:expr, $iter:ident, $r:ty, |$c:ident, $k:ident| $visit:expr) => {
        match $t {
            Type::Array(e)
            | Type::Nullable(e)
            | Type::Rest(e)
            | Type::MapIter(e)
            | Type::Promise(e) => {
                let ($c, $k): ($r, Part) = (e, Part::Held);
                $visit;
            }
            Type::Map(a, b) | Type::Dict(a, b) => {
                for e in [a, b] {
                    let ($c, $k): ($r, Part) = (e, Part::Held);
                    $visit;
                }
            }
            Type::Function(ps, ret) => {
                for p in ps.$iter() {
                    let ($c, $k): ($r, Part) = (p, Part::Param);
                    $visit;
                }
                let ($c, $k): ($r, Part) = (ret, Part::Held);
                $visit;
            }
            Type::Struct(fs) => {
                for (_, f) in fs.$iter() {
                    let ($c, $k): ($r, Part) = (f, Part::Held);
                    $visit;
                }
            }
            Type::Union(ts) => {
                for u in ts.$iter() {
                    let ($c, $k): ($r, Part) = (u, Part::Held);
                    $visit;
                }
            }
            Type::Number
            | Type::String
            | Type::Boolean
            | Type::Void
            | Type::Never
            | Type::BigInt
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::Set
            | Type::ArrIter
            | Type::Any
            | Type::Object(_)
            | Type::ClassRef(_)
            | Type::TypeVar(_)
            | Type::Null
            | Type::Undefined
            | Type::Literal(_)
            | Type::RegExp
            | Type::Date
            | Type::Symbol => {}
        }
    };
}

/// Whether `f` holds for some part of `t`.
pub(super) fn any_part(t: &Type, mut f: impl FnMut(&Type, Part) -> bool) -> bool {
    let mut hit = false;
    each_part!(t, iter, &Type, |c, k| hit = hit || f(c, k));
    hit
}

/// Run `f` on every part of `t`.
pub(super) fn each_part_mut(t: &mut Type, mut f: impl FnMut(&mut Type)) {
    each_part!(t, iter_mut, &mut Type, |c, _k| f(c));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(t: &Type) -> Vec<(Type, bool)> {
        let mut out = Vec::new();
        any_part(t, |c, k| {
            out.push((c.clone(), k == Part::Param));
            false
        });
        out
    }

    #[test]
    fn reaches_every_container() {
        let n = || Box::new(Type::Number);
        for t in [
            Type::Array(n()),
            Type::Nullable(n()),
            Type::Rest(n()),
            Type::MapIter(n()),
            Type::Promise(n()),
        ] {
            assert_eq!(parts(&t), vec![(Type::Number, false)]);
        }
        for t in [
            Type::Map(n(), Box::new(Type::Any)),
            Type::Dict(n(), Box::new(Type::Any)),
        ] {
            assert_eq!(parts(&t), vec![(Type::Number, false), (Type::Any, false)]);
        }
        let f = Type::Function(vec![Type::String], Box::new(Type::Any));
        assert_eq!(parts(&f), vec![(Type::String, true), (Type::Any, false)]);
        assert!(parts(&Type::ClassRef("H".into())).is_empty());
    }

    #[test]
    fn rewrites_every_part() {
        let mut t = Type::Dict(
            Box::new(Type::String),
            Box::new(Type::Promise(Box::new(Type::Number))),
        );
        fn deep(t: &mut Type) {
            if *t == Type::Number {
                *t = Type::Any;
            }
            each_part_mut(t, deep);
        }
        deep(&mut t);
        assert_eq!(
            t,
            Type::Dict(
                Box::new(Type::String),
                Box::new(Type::Promise(Box::new(Type::Any)))
            )
        );
    }
}
