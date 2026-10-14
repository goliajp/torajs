//! The slot type of a scalar / keyword annotation. Moved out of
//! [`super`].

use std::collections::HashMap;

use crate::ssa::Type;

/// Flat scalar / keyword annotation tail (`number` / `string` / `Map`
/// / ... plus the alias fallback) — split from `parse_type`
/// (2026-07-03: the chunk-437 split left the fn at 201 LOC, one over
/// the 200 hard limit; match body verbatim).
pub(super) fn parse_keyword(s: &str, aliases: &HashMap<String, Type>) -> Type {
    match s {
        // `number` defaults to i64 — best for the integer-heavy cases
        // (popcount/fib40/gcd1m). f64 is opt-in via explicit annotation;
        // matches TS where `number` is f64 but most user code stays in
        // safe-integer range. Bench code uses `number` and gets i64.
        "number" | "i64" => Type::I64,
        "f64" => Type::F64,
        "boolean" => Type::Bool,
        "string" => Type::Str,
        "void" => Type::Void,
        // a never fn returns only on its throw path, where the
        // pending-throw epilogue writes a 0 sentinel into an I64
        // slot; a Void slot would leave x0 undefined for callers
        // reading a pointer-shaped result through a wider face
        "never" => Type::I64,
        "regex" | "RegExp" => Type::RegExp,
        // `Date` is the TS-spelled annotation; `date` is the internal
        // spelling type_to_ann emits. Mirrors the check_type_ann arm.
        "date" | "Date" => Type::Date,
        // T-21 — `fetch(url)` Response heap struct; ptr at SSA.
        "Response" => Type::Ptr,
        // T-10.a — Any plumbing; single 64B ptr slot, tag via heap header.
        "any" => Type::Any,
        // a union's / dictionary's value is NaN-boxed: the members are a checker fact
        s if s.starts_with("__union(") || s.starts_with("__dict(") => Type::Any,
        // TS `object` collapses to Type::Any (non-primitive constraint
        // is independent substrate; mirror of `check_type_ann.rs`).
        "object" => Type::Any,
        // TS `unknown` — top type, alias of Type::Any at the runtime
        // layer (mirror of `check_type_ann.rs`). No-access-without-
        // narrow constraint is independent L3b.
        "unknown" => Type::Any,
        // the template object a tag receives (mirror of `check_type_ann.rs`)
        "TemplateStringsArray" => Type::Any,
        // T-13.a (v0.4.0) — Symbol value. Heap-allocated 16-byte
        // block, identity is pointer identity. Lowers to ptr.
        "symbol" => Type::Symbol,
        // T-25 (v0.7) — BigInt value. Heap-allocated sign-magnitude
        // struct (runtime_bigint.c). Lowers to ptr.
        "bigint" => Type::BigInt,
        // T-26 (v0.7) — WeakRef. Heap-allocated 16-byte struct.
        // `weakref` is type_to_ann's spelling; the TS spelling arrives
        // from a machine-lifted field (`const w = new WeakRef(t)` in
        // an async body) — the same pairs the checker's registry
        // (`check_type_ann`) accepts.
        "weakref" | "WeakRef" => Type::WeakRef,
        // P6.1 — strong-ref Map / Set. Type-erased keys + values via
        // the tagged-Any slot layout in runtime_map.c.
        "Map" | "map" => Type::Map,
        "Set" | "set" => Type::Set,
        // P6.4b — Map iterator handle returned by m.keys / .values / .entries.
        "mapiter" | "MapIter" => Type::MapIter,
        // P6.4c-C3 — Array<Any> iterator handle.
        "arriter" | "ArrIter" => Type::ArrIter,
        // T-26.B (v0.7) — WeakMap / WeakSet. Type-erased keys + values.
        "weakmap" | "WeakMap" => Type::WeakMap,
        "weakset" | "WeakSet" => Type::WeakSet,
        other => match aliases.get(other) {
            Some(ty) => *ty,
            None => panic!("ssa-lower: unsupported type annotation `{other}`"),
        },
    }
}
