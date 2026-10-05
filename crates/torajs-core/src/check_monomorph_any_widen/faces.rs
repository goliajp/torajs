//! The two faces a widened clone is cut along: what each widened param
//! slot becomes, and what the return becomes.

use crate::check::Type;

/// What a clone's return face becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetFace {
    /// The declared return: a call-site clone whose return does not
    /// read the widened params (`check_type_of_call::any_widen_ret`).
    Declared,
    /// A TypeVar return erases to `any` — the value-escape clone has
    /// no type params left to bind it to.
    EraseTypeVar,
    /// `any`: the body re-types under `any` params, so what it hands
    /// back is whatever it computed, not a cast of it to the declared
    /// type (`struct_escape`).
    Any,
}

/// What a widened param slot becomes: the array channel rewrites
/// `Array(Any)`-admitted params to `any[]`; the scalar channel (both
/// the value-escape TypeVar erase and the r380 `Any`-arg-into-
/// heap-param wedge) rewrites them to plain `any`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidenTarget {
    Arr,
    Scalar,
}

impl WidenTarget {
    pub(crate) fn ann(&self) -> &'static str {
        match self {
            WidenTarget::Arr => "any[]",
            WidenTarget::Scalar => "any",
        }
    }
    pub(super) fn ty(&self) -> Type {
        match self {
            WidenTarget::Arr => Type::Array(Box::new(Type::Any)),
            WidenTarget::Scalar => Type::Any,
        }
    }
    /// Per-index tag in the clone's name suffix. Two sites widening
    /// the SAME index to DIFFERENT targets must not collide on one
    /// mono name — the tag keeps `f$$anyw0a` and `f$$anyw0s` apart.
    pub(super) fn tag(&self) -> char {
        match self {
            WidenTarget::Arr => 'a',
            WidenTarget::Scalar => 's',
        }
    }
}
