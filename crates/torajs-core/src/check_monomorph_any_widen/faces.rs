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
/// heap-param wedge) rewrites them to plain `any`; the face channel
/// rewrites a fn-typed param to the signature of the function value
/// the call hands it (`check_type_of_call::fn_face`), so the clone's
/// calls through the param use the callee's own calling convention.
#[derive(Debug, Clone, PartialEq)]
pub enum WidenTarget {
    Arr,
    Scalar,
    /// A `Promise<T>` param handed a promise of `any`: the clone's slot
    /// is `Promise<any>`, so its awaits and handlers read the settled
    /// value on the any lanes.
    Promise,
    Face {
        /// The annotation the clone's param slot is spelled with: the
        /// original param's repr marker around the value's face.
        ann: String,
        /// The checker type of that face.
        ty: Type,
    },
}

impl WidenTarget {
    pub(crate) fn ann(&self) -> &str {
        match self {
            WidenTarget::Arr => "any[]",
            WidenTarget::Scalar => "any",
            WidenTarget::Promise => "Promise<any>",
            WidenTarget::Face { ann, .. } => ann,
        }
    }
    pub(super) fn ty(&self) -> Type {
        match self {
            WidenTarget::Arr => Type::Array(Box::new(Type::Any)),
            WidenTarget::Scalar => Type::Any,
            WidenTarget::Promise => Type::Promise(Box::new(Type::Any)),
            WidenTarget::Face { ty, .. } => ty.clone(),
        }
    }
    /// Per-index tag in the clone's name suffix. Two sites widening
    /// the SAME index to DIFFERENT targets must not collide on one
    /// mono name — the tag keeps `f$$anyw0a` and `f$$anyw0s` apart,
    /// and a face tag carries a hash of its annotation and its type so
    /// two faces on one index get two clones.
    pub(super) fn tag(&self) -> String {
        match self {
            WidenTarget::Arr => "a".to_string(),
            WidenTarget::Scalar => "s".to_string(),
            WidenTarget::Promise => "p".to_string(),
            WidenTarget::Face { ann, ty } => format!("f{:016x}", fnv1a(&format!("{ann}|{ty:?}"))),
        }
    }
}

/// FNV-1a over the spelling: build-deterministic, no dependency.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
