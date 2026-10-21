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

/// `<marker>(P1|...)->(R)` — the fn-type ann shape. `marker` picks the
/// repr `parse_type` interns: `__fn` = `Type::FnSig` (bare ptr, direct
/// dispatch) for param / return / let positions, `__cls` =
/// `Type::Closure` (env-first) for struct-field slots.
fn fn_ann(marker: &str, args: &[Type], ret: &Type) -> String {
    let parts: Vec<String> = args.iter().map(type_to_ann).collect();
    crate::type_ann_fnsig::fn_type_ann(marker, &parts.join("|"), &type_to_ann(ret))
}

/// `parse_type` consumes. Used to translate inferred generic type args
/// from the typechecker into ssa_lower's annotation strings.
pub fn type_to_ann(ty: &Type) -> String {
    match ty {
        Type::Number => "number".into(),
        Type::Boolean => "boolean".into(),
        Type::String => "string".into(),
        Type::Void => "void".into(),
        Type::Never => "never".into(),
        Type::BigInt => "bigint".into(),
        Type::WeakRef => "weakref".into(),
        Type::WeakMap => "weakmap".into(),
        Type::WeakSet => "weakset".into(),
        Type::Map(..) => "Map".into(),
        Type::Set => "Set".into(),
        Type::MapIter(..) => "mapiter".into(),
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
                .map(|(n, ft)| match ft {
                    // RFC 20260710 C4 — FIELD position keeps the
                    // nullable wrapper: a `__nullable(number|boolean)`
                    // slot is Any at SSA (per-type undefined repr), so
                    // collapsing to the bare inner ann would intern a
                    // raw-scalar twin layout diverging from the
                    // TypeDecl-registered one. Non-field positions
                    // keep the historical collapse (Nullable arm
                    // below) — their storage is repr-identical to T.
                    // A fn-typed FIELD is a Closure-repr slot, mirroring
                    // `tag_struct_field_closure_types`'s retag on the
                    // annotated path: the slot can hold a capturing
                    // closure, so it interns as `Type::Closure`
                    // (env-first CallIndirect). `__fn(` would intern a
                    // bare-fn-ptr slot and the call site would direct-
                    // dispatch a closure pair — SIGBUS. Only the
                    // annotated path was tagged; an INFERRED struct
                    // (`function make() { return { f: () => 7 } }`)
                    // reaches SSA through here.
                    Type::Nullable(inner) => match &**inner {
                        Type::Function(args, ret) => {
                            format!("{}:__nullable({})", n.lossy(), fn_ann("__cls", args, ret))
                        }
                        _ => format!("{}:__nullable({})", n.lossy(), type_to_ann(inner)),
                    },
                    Type::Function(args, ret) => {
                        format!("{}:{}", n.lossy(), fn_ann("__cls", args, ret))
                    }
                    other => format!("{}:{}", n.lossy(), type_to_ann(other)),
                })
                .collect();
            format!("__struct({})", parts.join("|"))
        }
        Type::Function(args, ret) => fn_ann("__fn", args, ret),
        // A builtin namespace / constructor VALUE (`Array`, `Math`, `Map`
        // passed as an argument) has no SSA spelling of its own: its name
        // is either unknown to `parse_type` (`Array` panicked) or names
        // the INSTANCE type (`Map` would type the constructor as a Map).
        // The value lowers as `any`, which is what an explicit
        // `o: any` parameter already does.
        Type::Object(name) if crate::check_type_of_ident::NS_OBJECT_IDENTS.contains(name) => {
            "any".into()
        }
        Type::Object(name) => (*name).into(),
        // A class reference IS its own canonical annotation — the class
        // name — and ssa_lower::parse_type resolves it against the SSA
        // layer's own alias table. This covers both the generic
        // back-edge (`Rec<number>`, which re-instantiates on that side)
        // and, since RFC 20260715-nominal-class-identity, every ordinary
        // class instance: its checked type is now `ClassRef(C)` rather
        // than the field struct it used to collapse to. Emitting
        // `__struct(...)` here instead would strip the class name off
        // the SSA type and undo the nominal identity.
        Type::ClassRef(name) => name.clone(),
        Type::TypeVar(_) => {
            panic!("type_to_ann: TypeVar should be substituted before SSA layer")
        }
        // SSA layer treats nullable as the underlying T (storage and
        // call boundaries are identical — the only difference is that
        // `null` is a legal value of T). The annotation collapses to
        // T's annotation; check.rs is the only layer that distinguishes
        // them, and it's already past by the time this fn runs.
        Type::Nullable(inner) => type_to_ann(inner),
        // a union's, a literal's and a dictionary's value travels as
        // its lane: the any box, or the one type a union's literals widen to
        Type::Union(_) | Type::Dict(..) | Type::Literal(_) => type_to_ann(&ty.clone().lane()),
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
        // RFC 20260708-variadic — re-encode the rest sentinel in its
        // marker spelling (round-trips through both decode sites).
        Type::Rest(elem) => format!("__rest({}[])", type_to_ann(elem)),
    }
}

/// [`type_to_ann`] for the slot a settled promise value is read into.
/// A `T | null` / `T | undefined` keeps its `__nullable(…)` spelling,
/// at the top and as an array element, so `parse_type` answers the
/// slot a binding annotated with that union gets: the any box for a
/// scalar `T` (a raw number has no spelling for undefined), and the any
/// lane for every element (`(T | null)[]`, which is also the lane a
/// `Promise.all` of such promises builds its result in). Collapsed to
/// `T`, an undefined settled value read back as `NaN`.
pub fn type_to_value_ann(ty: &Type) -> String {
    match ty {
        Type::Nullable(inner) => format!("__nullable({})", type_to_ann(inner)),
        Type::Array(elem) => format!("{}[]", type_to_value_ann(elem)),
        other => type_to_ann(other),
    }
}
