//! Union types (TS §3.4): the canonical constructor, and the fold that
//! hands a union's value to its consumers as `Any`.
//!
//! A union's members are a checker-side fact — what may be assigned
//! into a union slot, and which members every value of it has. Its
//! value is not typed by any one member, so at runtime it is the
//! NaN-boxed `Any`, the representation a scalar `T | null` already
//! uses. Hence two views of one type: [`Type::union_of`] keeps the
//! members for the slot checks, and [`Type::lane`] answers the type the
//! value travels as.

use super::Type;

impl Type {
    /// The union of `members`, normalized: nested unions flattened,
    /// `Nullable(T)` split into `T` plus the nullish pair, `never`
    /// dropped, duplicates removed, the rest sorted so that member
    /// order never makes two unions differ. `any` absorbs the union,
    /// and when one non-nullish member is left the answer is that
    /// member — wrapped in `Nullable` if a nullish one was present,
    /// the single-member shape the rest of the checker knows.
    pub(crate) fn union_of(members: impl IntoIterator<Item = Type>) -> Type {
        let mut acc = Acc::default();
        for m in members {
            acc.add(m);
        }
        acc.finish()
    }

    /// The type a value of this type travels as: every union and every
    /// dictionary ([`Type::Dict`]), at any depth, folded to `Any`. A union's value rides the any lane, so
    /// an array of unions is an `any[]`, a struct field holding one is
    /// an `any` field, and so on — the shapes the lowering already
    /// builds for those annotations.
    pub(crate) fn lane(self) -> Type {
        if self.has_lane_fold() {
            self.fold()
        } else {
            self
        }
    }

    /// The type a declared slot keeps: a union at the top keeps its
    /// members (each folded), every union below the top is folded.
    /// Only a binding, a parameter and a return type are checked
    /// member by member; a union inside an array, a field or a
    /// generic argument is the any-typed slot the lowering builds
    /// for it.
    pub(crate) fn decl_view(self) -> Type {
        match self {
            Type::Union(ms) => Type::Union(ms.into_iter().map(Type::member_view).collect()),
            t @ Type::Literal(_) => t,
            // the value type keeps its own top union for the write check
            Type::Dict(k, v) => Type::Dict(k, Box::new(v.decl_view())),
            t => t.lane(),
        }
    }

    /// A union member as a slot keeps it: a literal, and an object
    /// member's literal-typed field, stay — they are what tells the
    /// members apart — while every union below is folded.
    fn member_view(self) -> Type {
        match self {
            Type::Struct(fields) => Type::Struct(
                fields
                    .into_iter()
                    .map(|(k, t)| (k, t.field_view()))
                    .collect(),
            ),
            t => t.field_view(),
        }
    }

    /// An object type's field as its declaration keeps it: a literal
    /// type stays, anything else is its lane.
    pub(crate) fn field_view(self) -> Type {
        match self {
            t @ Type::Literal(_) => t,
            t => t.lane(),
        }
    }

    /// This type with every literal type replaced by its base — the
    /// type of the annotation the rest of the program reads, which
    /// spells each literal type as its base
    /// (`check_type_ann::widen_literal_types`).
    pub(crate) fn erase_literals(self) -> Type {
        match self {
            Type::Literal(l) => l.base(),
            Type::Struct(fields) => Type::Struct(
                fields
                    .into_iter()
                    .map(|(k, t)| (k, t.erase_literals()))
                    .collect(),
            ),
            Type::Union(ms) => Type::union_of(ms.into_iter().map(Type::erase_literals)),
            Type::Array(e) => Type::Array(Box::new(e.erase_literals())),
            Type::Nullable(e) => Type::Nullable(Box::new(e.erase_literals())),
            t => t,
        }
    }

    /// The narrow of a binding declared as `declared` down to
    /// `members`. The binding's slot is what its declaration lowers to;
    /// when that is the any box while the narrowed members alone would
    /// travel typed (`"a" | number` narrowed to `"a"`), an `Any` member
    /// keeps the lane at `Any` — it has every member and fits every
    /// slot, so the member and assignability checks read the others.
    pub(crate) fn union_narrowed(members: Vec<Type>, declared: &[Type]) -> Type {
        let mut narrowed = members;
        let slot_is_any = Type::Union(declared.to_vec()).lane() == Type::Any;
        if slot_is_any && Type::Union(narrowed.clone()).lane() != Type::Any {
            narrowed.push(Type::Any);
        }
        Type::Union(narrowed)
    }

    /// TS §3.11.4 assignability with a union on either side: a union
    /// value fits a slot when every member does, a union slot takes
    /// what any one of its members takes. A dictionary slot is
    /// [`Type::dict_fits`]'s. `None` when neither rule applies; `fits`
    /// is the caller's own member test.
    pub(crate) fn lane_fits(
        to: &Type,
        from: &Type,
        fits: &mut dyn FnMut(&Type, &Type) -> bool,
    ) -> Option<bool> {
        match (to, from) {
            (_, Type::Union(ms)) => Some(ms.iter().all(|m| fits(to, m))),
            (Type::Literal(a), Type::Literal(b)) => Some(a == b),
            // a value of the base type whose literal the checker did not
            // keep (an object literal's field, a widened binding) is
            // taken: this under-refuses where tsc tracks the fresh
            // literal, and never refuses a program tsc accepts
            (Type::Literal(l), _) => Some(*from == l.base()),
            (_, Type::Literal(l)) if !matches!(to, Type::Union(_)) => Some(fits(to, &l.base())),
            // `T | null | undefined` fits when each of its three does
            (Type::Union(ms), Type::Nullable(inner)) => Some(
                [(**inner).clone(), Type::Null, Type::Undefined]
                    .iter()
                    .all(|p| ms.iter().any(|m| fits(m, p))),
            ),
            (Type::Union(ms), _) => Some(ms.iter().any(|m| fits(m, from))),
            _ => Type::dict_fits(to, from, fits),
        }
    }

    /// TS §3.11.4 assignability into a dictionary slot `{ [k]: V }`: a
    /// dictionary fits when its value type does; an object type when
    /// every one of its properties fits `V` (TS §3.11.4's index
    /// signature rule, with the object literal's implicit signature);
    /// an array when its element type does (its keys are indices).
    /// `None` for any other pair.
    pub(crate) fn dict_fits(
        to: &Type,
        from: &Type,
        fits: &mut dyn FnMut(&Type, &Type) -> bool,
    ) -> Option<bool> {
        let Type::Dict(k, v) = to else { return None };
        match from {
            Type::Dict(_, w) | Type::Array(w) => Some(fits(v, w)),
            Type::Struct(fields) => Some(
                fields
                    .iter()
                    .all(|(n, t)| !k.dict_key_covers(&n.lossy()) || fits(v, t)),
            ),
            _ => None,
        }
    }

    /// Whether a union or a dictionary occurs anywhere in this type —
    /// the common answer is no, and then neither view rebuilds the type.
    pub(crate) fn has_lane_fold(&self) -> bool {
        match self {
            Type::Union(_) | Type::Dict(..) | Type::Literal(_) => true,
            Type::Array(e) | Type::Rest(e) | Type::Promise(e) | Type::MapIter(e) => {
                e.has_lane_fold()
            }
            Type::Nullable(e) => e.has_lane_fold(),
            Type::Map(k, v) => k.has_lane_fold() || v.has_lane_fold(),
            Type::Function(ps, r) => ps.iter().any(Type::has_lane_fold) || r.has_lane_fold(),
            Type::Struct(fields) => fields.iter().any(|(_, t)| t.has_lane_fold()),
            _ => false,
        }
    }

    fn fold(self) -> Type {
        match self {
            // the members a literal type tells apart are one value
            // type once their literals are erased: that type is the
            // lane, the one the lowering reads off the same annotation
            Type::Union(ms) => match Type::union_of(ms.into_iter().map(Type::erase_literals)) {
                Type::Union(_) => Type::Any,
                t => t.lane(),
            },
            Type::Literal(l) => l.base(),
            Type::Dict(..) => Type::Any,
            Type::Array(e) => Type::Array(Box::new(e.lane())),
            Type::Rest(e) => Type::Rest(Box::new(e.lane())),
            Type::Promise(e) => Type::Promise(Box::new(e.lane())),
            Type::MapIter(e) => Type::MapIter(Box::new(e.lane())),
            Type::Map(k, v) => Type::Map(Box::new(k.lane()), Box::new(v.lane())),
            Type::Function(ps, r) => {
                Type::Function(ps.into_iter().map(Type::lane).collect(), Box::new(r.lane()))
            }
            Type::Struct(fields) => {
                Type::Struct(fields.into_iter().map(|(k, t)| (k, t.lane())).collect())
            }
            Type::Nullable(inner) => match inner.lane() {
                Type::Any => Type::Any,
                t => Type::Nullable(Box::new(t)),
            },
            t => t,
        }
    }
}

#[derive(Default)]
struct Acc {
    members: Vec<Type>,
    null: bool,
    undefined: bool,
    any: bool,
}

impl Acc {
    fn add(&mut self, t: Type) {
        match t {
            Type::Union(ms) => ms.into_iter().for_each(|m| self.add(m)),
            Type::Nullable(inner) => {
                self.null = true;
                self.undefined = true;
                self.add(*inner);
            }
            Type::Null => self.null = true,
            Type::Undefined => self.undefined = true,
            Type::Never => {}
            Type::Any => self.any = true,
            t => {
                if !self.members.contains(&t) {
                    self.members.push(t);
                }
            }
        }
    }

    fn finish(mut self) -> Type {
        if self.any {
            return Type::Any;
        }
        let nullish = self.null || self.undefined;
        match self.members.len() {
            0 => match (self.null, self.undefined) {
                (true, true) => Type::Nullable(Box::new(Type::Null)),
                (true, false) => Type::Null,
                (false, true) => Type::Undefined,
                (false, false) => Type::Never,
            },
            1 => {
                let m = self.members.pop().expect("one member");
                if nullish {
                    Type::Nullable(Box::new(m))
                } else {
                    m
                }
            }
            _ => {
                self.members.sort_by_cached_key(|m| format!("{m:?}"));
                if nullish {
                    self.members.push(Type::Null);
                    self.members.push(Type::Undefined);
                }
                Type::Union(self.members)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Type;

    #[test]
    fn member_order_does_not_matter() {
        let a = Type::union_of([Type::String, Type::Number]);
        let b = Type::union_of([Type::Number, Type::String, Type::Number]);
        assert_eq!(a, b);
        assert!(matches!(a, Type::Union(ref ms) if ms.len() == 2));
    }

    #[test]
    fn nested_unions_flatten_and_any_absorbs() {
        let inner = Type::union_of([Type::String, Type::Number]);
        let outer = Type::union_of([inner.clone(), Type::Boolean]);
        assert!(matches!(outer, Type::Union(ref ms) if ms.len() == 3));
        assert_eq!(Type::union_of([inner, Type::Any]), Type::Any);
    }

    #[test]
    fn one_member_left_is_that_member() {
        assert_eq!(
            Type::union_of([Type::Number, Type::Null]),
            Type::Nullable(Box::new(Type::Number))
        );
        let nn = Type::Nullable(Box::new(Type::Nullable(Box::new(Type::String))));
        assert_eq!(
            Type::union_of([nn, Type::Undefined]),
            Type::Nullable(Box::new(Type::String))
        );
    }

    #[test]
    fn nullish_member_brings_both_nullish_values() {
        let u = Type::union_of([Type::String, Type::Number, Type::Null]);
        assert!(matches!(u, Type::Union(ref ms) if ms.contains(&Type::Undefined)));
    }

    #[test]
    fn lane_folds_every_depth_and_decl_view_keeps_the_top() {
        let u = Type::union_of([Type::String, Type::Number]);
        let arr = Type::Array(Box::new(u.clone()));
        assert_eq!(arr.clone().lane(), Type::Array(Box::new(Type::Any)));
        assert_eq!(u.clone().lane(), Type::Any);
        assert_eq!(u.clone().decl_view(), u);
        assert_eq!(arr.decl_view(), Type::Array(Box::new(Type::Any)));
    }
}
