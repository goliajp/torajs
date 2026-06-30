//! `type_to_ann` pulled out of [`crate::check`] as chunk-314 of
//! the check.rs god-file decomp. Pure derivation
//! `&Type -> String` — no mutation, no checker state. Used by
//! `ssa_lower` to translate inferred generic type args (and other
//! Type values) into the annotation strings that `parse_type`
//! consumes round-trip.
//!
//! Re-exported from `check` as `pub use check_type_to_ann::type_to_ann;`
//! so callers using `crate::check::type_to_ann` (the canonical
//! import path) continue to work without churn.

use crate::check::Type;

/// `parse_type` consumes. Used to translate inferred generic type args
/// from the typechecker into ssa_lower's annotation strings.
pub fn type_to_ann(ty: &Type) -> String {
    match ty {
        Type::Number => "number".into(),
        Type::Boolean => "boolean".into(),
        Type::String => "string".into(),
        Type::Void => "void".into(),
        Type::BigInt => "bigint".into(),
        Type::WeakRef => "weakref".into(),
        Type::WeakMap => "weakmap".into(),
        Type::WeakSet => "weakset".into(),
        Type::Map => "Map".into(),
        Type::Set => "Set".into(),
        Type::MapIter => "mapiter".into(),
        Type::ArrIter => "arriter".into(),
        // T-28-substrate — SSA Type::Any is its own slot type at the
        // SSA layer (parse_type's "any" round-trips to Type::Any).
        // Pre-T-28-substrate this collapsed to "number" because Any-
        // typed flows weren't fully wired through the SSA layer; the
        // collapse silently corrupted padded ANY_UNDEF Any-box ptrs
        // when stuffed into i64 Number slots. Round-tripping as "any"
        // gives generic mono its own Any specialization.
        Type::Any => "any".into(),
        Type::Symbol => "symbol".into(),
        Type::Array(inner) => format!("{}[]", type_to_ann(inner)),
        // Structs encode structurally as `__struct(field_name1:T1|...)`.
        // ssa_lower's `parse_type` decodes the same shape, looks up
        // (or interns) the matching `Type::Obj(StructId)`. Each
        // distinct struct shape produces a distinct annotation so the
        // generic mono cache no longer collides on `void`.
        Type::Struct(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(n, ft)| format!("{n}:{}", type_to_ann(ft)))
                .collect();
            format!("__struct({})", parts.join("|"))
        }
        Type::Function(args, ret) => {
            let parts: Vec<String> = args.iter().map(type_to_ann).collect();
            format!("__fn({})->{}", parts.join("|"), type_to_ann(ret))
        }
        Type::Object(name) => (*name).into(),
        Type::ClassRef(name) => {
            // Generic-instantiation back-edge (`Rec<number>`): the key
            // IS its own canonical annotation — emit it verbatim and
            // ssa_lower::parse_type re-instantiates on its side.
            if name.contains('<') {
                return name.clone();
            }
            /* V3-05 — non-generic class references should have been
             * resolved (via aliases lookup) before reaching ssa_lower.
             * The placeholder Pass should have been replaced by the
             * Real Type::Struct in c.aliases by the time anyone
             * asks for an SSA annotation. Panic to surface stale
             * usage rather than silently emitting `__struct()` for
             * a class. */
            panic!(
                "type_to_ann: ClassRef(`{name}`) reached SSA-ann emission — caller should have called resolve_class_ref first"
            )
        }
        Type::TypeVar(_) => {
            panic!("type_to_ann: TypeVar should be substituted before SSA layer")
        }
        // SSA layer treats nullable as the underlying T (storage and
        // call boundaries are identical — the only difference is that
        // `null` is a legal value of T). The annotation collapses to
        // T's annotation; check.rs is the only layer that distinguishes
        // them, and it's already past by the time this fn runs.
        Type::Nullable(inner) => type_to_ann(inner),
        Type::Null => "null".into(),
        // P1.1 — Type::Undefined collapses to `null` in the SSA-ann
        // string for now since the SSA layer has no separate Undefined.
        // The runtime tag (ANY_NULL=0 vs ANY_UNDEF=5) is the actual
        // disambiguator and lives in the box helpers; the static SSA
        // type stays Ptr-shaped for both.
        Type::Undefined => "undefined".into(),
        // RegExp is its own SSA type (Type::RegExp); the annotation
        // round-trips through ssa_lower's parse_type back to the same.
        Type::RegExp => "regex".into(),
        Type::Date => "date".into(),
        Type::Promise(inner) => format!("Promise<{}>", type_to_ann(inner)),
    }
}
