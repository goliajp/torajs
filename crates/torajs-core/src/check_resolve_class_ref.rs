//! `resolve_class_ref` + private helper `resolve_class_ref_one`
//! pulled out of [`crate::check`] as chunk-316 of the check.rs
//! god-file decomp.
//!
//! Class-reference unwrap — given a `Type::ClassRef("Node")`
//! placeholder (minted at parse time for forward references and
//! self-referential class fields), return the underlying
//! `Type::Struct(...)` shape one layer at a time. Idempotent:
//! resolving an already-resolved Type is a no-op. Generic
//! back-edges (`Rec<number>`) lazily re-instantiate one shallow
//! layer through `check_type_ann::resolve_type_ann_full`. Wrapper
//! variants (Nullable / Array) recurse via the public entry; their
//! inner ClassRef nodes get resolved on the next access (keeps
//! recursive class layouts finite — a fully-resolved `Node` would
//! expand infinitely).
//!
//! Re-exported from `check` as
//! `pub use check_resolve_class_ref::resolve_class_ref;` so the
//! external callers (`check_assignable` / `check_assign_target` /
//! `check_type_of_member`) continue to use the canonical
//! `crate::check::resolve_class_ref` import path.

use std::borrow::Cow;

use crate::check::{GenericAliasMap, Type};

/// `resolve_class_ref` walks one layer of class-reference indirection.
/// See [module doc](crate::check_resolve_class_ref) for the full
/// rationale + recursion invariant.
pub fn resolve_class_ref(
    ty: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> Type {
    resolve_class_ref_cow(ty, class_structs, aliases, generic_aliases).into_owned()
}

/// Borrow-preserving flavor of [`resolve_class_ref`] — the common
/// already-resolved input (a plain `Struct` / primitive) answers
/// `Cow::Borrowed` instead of deep-cloning the whole shape. A
/// wide-struct program pays that clone per member access: the 75KB
/// test262 unicode-ident class file spent a double-digit slice of its
/// 21s checker stall re-cloning one thousands-of-fields Struct
/// (rotation 268 profile). Only the transforming arms (`ClassRef`
/// unwrap, `Nullable` / `Array` inner recursion) allocate.
pub fn resolve_class_ref_cow<'t>(
    ty: &'t Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> Cow<'t, Type> {
    match ty {
        Type::ClassRef(_) => Cow::Owned(resolve_class_ref_unwrap(
            ty,
            class_structs,
            aliases,
            generic_aliases,
        )),
        Type::Nullable(_) | Type::Array(_) => Cow::Owned(resolve_class_ref_one(
            ty,
            class_structs,
            aliases,
            generic_aliases,
        )),
        _ => Cow::Borrowed(ty),
    }
}

/// The `ClassRef` unwrap arm of [`resolve_class_ref`] (body verbatim
/// from the pre-Cow entry).
fn resolve_class_ref_unwrap(
    ty: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> Type {
    match ty {
        Type::ClassRef(name) => {
            // RFC 20260715-nominal-class-identity — a declared class
            // stays `ClassRef(C)` in `aliases`; its shape lives in
            // `class_structs`. Everything else (forward refs, generic
            // back-edges) still resolves out of `aliases`.
            match class_structs.get(name).or_else(|| aliases.get(name)) {
                Some(t) if !matches!(t, Type::ClassRef(_)) => {
                    // Recurse: the alias entry's own fields may
                    // themselves contain ClassRef placeholders (the
                    // self-ref case — Node's `next` field carries
                    // ClassRef("Node")). One unwrap pass keeps
                    // following levels resolved at access time.
                    let resolved = t.clone();
                    resolve_class_ref_one(&resolved, class_structs, aliases, generic_aliases)
                }
                // Generic-instantiation back-edge (`Rec<number>` from a
                // recursive `type Rec<T>`): the key is not in `aliases`
                // (instantiation is lazy, there is no Pass-0 entry) but
                // it IS its own canonical annotation — re-instantiate
                // one shallow layer. Recursive fields inside come back
                // as ClassRef again, so this terminates: same lazy
                // one-layer-per-access contract as the named-class arm.
                // The FORCE-expand entry (blade 3a) bypasses the
                // resolver's nominal short-circuit, which would answer
                // this same ClassRef back and never make progress.
                None if name.contains('<') => {
                    match crate::check_type_ann::expand_instantiation_full(
                        name,
                        aliases,
                        &[],
                        generic_aliases,
                    ) {
                        Some(t) if !matches!(t, Type::ClassRef(_)) => t,
                        _ => ty.clone(),
                    }
                }
                _ => ty.clone(),
            }
        }
        _ => resolve_class_ref_one(ty, class_structs, aliases, generic_aliases),
    }
}

/// Helper: walk every wrapper variant once, leaving ClassRef nodes
/// embedded in struct/array fields alone (they get resolved on the
/// next access). This keeps recursive class layouts finite — a
/// fully-resolved Node would expand infinitely.
fn resolve_class_ref_one(
    ty: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> Type {
    match ty {
        Type::Nullable(inner) => Type::Nullable(Box::new(resolve_class_ref(
            inner,
            class_structs,
            aliases,
            generic_aliases,
        ))),
        Type::Array(inner) => Type::Array(Box::new(resolve_class_ref(
            inner,
            class_structs,
            aliases,
            generic_aliases,
        ))),
        _ => ty.clone(),
    }
}
