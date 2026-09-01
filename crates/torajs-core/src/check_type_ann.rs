//! Type-annotation string resolution — `resolve_type_ann_full` and
//! its in-flight-guarded recursive core, split out of check.rs. The
//! in-flight set names every generic instantiation currently being
//! expanded so a recursive `type Rec<T> = { next: Rec<T> | null }`
//! closes its back-edge as nominal `Type::ClassRef("Rec<number>")`
//! instead of recursing forever (V3-05 placeholder scheme mirror).

mod generic;
mod markers;
pub(crate) use generic::expand_instantiation_full;
pub(crate) use markers::split_top_pipe;
use std::collections::HashMap;

use crate::check::{GenericAliasMap, Type};

pub(crate) fn resolve_type_ann_full(
    name: &str,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
) -> Option<Type> {
    let mut in_flight = std::collections::HashSet::new();
    resolve_type_ann_inner(name, aliases, type_params, generic_aliases, &mut in_flight)
}

fn resolve_type_ann_inner(
    name: &str,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
    in_flight: &mut std::collections::HashSet<String>,
) -> Option<Type> {
    if let Some(rest) = name.strip_suffix("[]") {
        return resolve_type_ann_inner(rest, aliases, type_params, generic_aliases, in_flight)
            .map(|inner| Type::Array(Box::new(inner)));
    }
    // Nullable wrapper produced by the parser when it sees `T | null`.
    if let Some(rest) = name.strip_prefix("__nullable(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return resolve_type_ann_inner(inner, aliases, type_params, generic_aliases, in_flight)
            .map(|t| Type::Nullable(Box::new(t)));
    }
    if name == "null" {
        return Some(Type::Null);
    }
    // The `undefined` type (the type whose only value is `undefined`).
    // Represented like undefined values themselves (a null-shaped slot,
    // per the SSA `parse_type` "undefined" arm).
    if name == "undefined" {
        return Some(Type::Undefined);
    }
    // Rest-param marker produced by the parser for `...args: E[]`
    // inside a fn-type annotation (RFC 20260708-variadic). The inner
    // ann is the ARRAY spelling; the sentinel holds the element.
    if let Some(rest) = name.strip_prefix("__rest(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return resolve_type_ann_inner(inner, aliases, type_params, generic_aliases, in_flight)
            .and_then(|t| match t {
                Type::Array(elem) => Some(Type::Rest(elem)),
                _ => None,
            });
    }
    // `Head<args>` generic spellings — builtin generics
    // (Array / ReadonlyArray / Iterable / IteratorResult / Iterator /
    // IterableIterator), user generic-alias instantiation, Promise<T>,
    // and erased container generics (Map / Set / Weak* / *Iter). Both
    // pre-split guard blocks merged into generic::resolve_generic with
    // arm order (and fall-through semantics) preserved verbatim.
    if let Some(open_idx) = name.find('<')
        && name.ends_with('>')
        && !name.starts_with("__fn(")
        && !name.starts_with("__cls(")
        && !name.starts_with("__env(")
    {
        return generic::resolve_generic(
            name,
            open_idx,
            aliases,
            type_params,
            generic_aliases,
            in_flight,
        );
    }
    // M2 — closure env marker `__env(cap0|cap1|...)` injected by
    // `lift_arrow_fns` on the hidden first param of capturing arrows. At
    // the typechecker layer the env is just a printable opaque value
    // (capture types are tracked separately in `Checker.closure_captures`),
    // so we resolve it to `Any`. The SSA lowerer recognizes the same
    // marker string and emits the actual env load preamble.
    if name.starts_with("__env(") && name.ends_with(')') {
        return Some(Type::Any);
    }
    // RFC 20260708-closure-argv-face — raw argv pointer marker on
    // the synthetic `__torajs_argv` param. Opaque at the checker
    // layer (only the synthetic materialize call consumes it); the
    // SSA parse_type maps it to Ptr.
    if name == "__argvptr()" {
        return Some(Type::Any);
    }
    // V3-18 P2.4.c.2 — inline obj type `__inlobj(name1:T1|name2:T2|...)`
    // decoded in markers::resolve_inlobj.
    if let Some(rest) = name.strip_prefix("__inlobj(") {
        return markers::resolve_inlobj(rest, aliases, type_params, generic_aliases, in_flight);
    }
    // `__fn(P1|P2|...)->(R)` / `__cls(P1|...)->(R)` fn-type markers decoded
    // in markers::resolve_fn_cls.
    if let Some(rest) = name
        .strip_prefix("__fn(")
        .or_else(|| name.strip_prefix("__cls("))
    {
        return markers::resolve_fn_cls(rest, aliases, type_params, generic_aliases, in_flight);
    }
    // RFC 20260714-objlit-accessor blade 1 — `__mth(P1|...)->(R)`, an
    // object-literal METHOD slot. Types exactly like `__cls(`: the
    // receiver is not in the signature (it is the lifted fn's hidden
    // `__this`, reassembled at the field-call arm), so `o.m(x)` types at
    // the arity the source has. The prefix exists only to tell that call
    // arm to push the receiver — the `__cls(`-marker idiom.
    if let Some(rest) = name.strip_prefix("__mth(") {
        return markers::resolve_fn_cls(rest, aliases, type_params, generic_aliases, in_flight);
    }
    // M3 — bare identifier matching an in-scope type-param resolves to a
    // TypeVar regardless of any conflicting alias / primitive.
    if type_params.iter().any(|p| p == name) {
        return Some(Type::TypeVar(name.to_string()));
    }
    match name {
        // `number` is the JS-spelled umbrella; `i64` and `f64` are explicit
        // Rust-shaped aliases. The typechecker treats all three as the same
        // numeric category — the SSA lowerer is what actually distinguishes
        // i64 vs f64 representation per `parse_type` in ssa_lower.rs.
        "number" | "i64" | "f64" => Some(Type::Number),
        "string" => Some(Type::String),
        "boolean" => Some(Type::Boolean),
        "void" => Some(Type::Void),
        "bigint" => Some(Type::BigInt),
        // `RegExp` is the TS-spelled annotation for the regex value
        // type (`const re: RegExp = /a/`); `regex` is the internal
        // spelling type_to_ann emits, accepted for round-tripping.
        "regex" | "RegExp" => Some(Type::RegExp),
        // `Date` is the TS-spelled annotation for `Type::Date`;
        // `date` is the internal spelling type_to_ann emits, accepted
        // for round-tripping. Without this arm, `class W { d: Date }`
        // is rejected as "unknown type Date" because class-field ann
        // resolution flows through this registry rather than the
        // user-class lookup. Date method dispatch (getTime / getUTC*
        // / toISOString …) already exists on `Type::Date` in check.rs.
        "date" | "Date" => Some(Type::Date),
        "weakref" | "WeakRef" => Some(Type::WeakRef),
        "weakmap" | "WeakMap" => Some(Type::WeakMap),
        "weakset" | "WeakSet" => Some(Type::WeakSet),
        "Map" => Some(Type::Map),
        "Set" => Some(Type::Set),
        "mapiter" | "MapIter" => Some(Type::MapIter),
        "arriter" | "ArrIter" => Some(Type::ArrIter),
        // `any` is recognized as a real type in the resolver only as a
        // late-stage fallback — `desugar_implicit_generics` rewrites
        // every annotated `: any` to a fresh TypeVar before this layer
        // sees it. A bare `any` reaching here means the AST pre-pass
        // was bypassed (e.g. a custom front-end test wiring), and we
        // accept it rather than reject so the surface stays self-
        // consistent.
        "any" => Some(Type::Any),
        // TS `object` — the non-primitive heap-shape (per TS spec
        // §3.2.8: any value except `null` / `undefined` / `number` /
        // `string` / `boolean` / `bigint` / `symbol`). The subset
        // collapses it to `Type::Any` since the "non-primitive only"
        // narrowing constraint is independent substrate work
        // (typecheck would need to reject `let x: object = 5`); for
        // ann-resolution surface this lets `WeakMap<object, V>` keys
        // and common `function f(o: object)` signatures parse without
        // forcing users into `any`. Independent L3b: enforce the
        // non-primitive constraint at assignment-check time.
        "object" => Some(Type::Any),
        // TS `unknown` — top type (per TS §2.7.2): any value, but
        // the type-checker rejects member access / arithmetic without
        // a narrowing guard first. The subset collapses to `Type::Any`
        // — runtime behaviour is identical, and the narrowing
        // requirement is independent substrate work (mirror of the
        // `object` non-primitive constraint above). Enables common
        // patterns like `function isStr(x: unknown): x is string` and
        // `const v: unknown = JSON.parse(s)` without forcing `any`.
        // L3b: enforce the no-access-without-narrow at member-access
        // / type-guard checking time.
        "unknown" => Some(Type::Any),
        // TS `Function` — the top callable type (any signature, any
        // return). Collapses to `Type::Any` like `object` / `unknown`
        // above: calls ride the proven any-call runtime dispatch
        // (RFC 20260704), which handles every closure shape. The
        // rest-tail alternative (`(...args: any[]) => any`) was
        // probed and mis-packs a FIXED-arity closure stored into the
        // variadic slot (no adapter face yet — recorded L3b); the
        // "only callables assignable" narrowing constraint is the
        // same independent substrate work as object's non-primitive
        // check.
        "Function" => Some(Type::Any),
        // RFC 20260823-typedarray-substrate — the buffer-family
        // classes annotate as `Type::Any` for the same reason their
        // constructors produce it: the cells are reached only
        // through the any-lane kernels in this slab.
        "ArrayBuffer" | "DataView" => Some(Type::Any),
        n2 if crate::ssa_lower_call_typedarray::kind_of_name(n2).is_some() => Some(Type::Any),
        // T-13.a (v0.4.0) — `symbol` is a primitive type alias for
        // Type::Symbol. Lower-case `symbol` is the spec spelling
        // (`typeof Symbol() === "symbol"`); `Symbol` is the constructor
        // function, not a type. Annotation `let s: symbol = Symbol()`
        // and `symbol[]` arrays both go through here.
        "symbol" => Some(Type::Symbol),
        // T-21 (v0.6.0) — `Response` is the heap struct returned by
        // `fetch(url)`. Its surface (.text() / .status) is wired in
        // the method-table arm; the type-resolver entry lets users
        // write `let r: Response = await fetch(url)` explicitly.
        "Response" => Some(Type::Object("Response")),
        // User-declared struct alias (P2.4): `type Point = { x: number, y: number }`
        // adds `Point` to the aliases map. Resolution returns the
        // structural Type::Struct directly — no nominal layer above.
        other => aliases.get(other).cloned(),
    }
}
