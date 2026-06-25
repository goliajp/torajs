//! Type-annotation string resolution — `resolve_type_ann_full` and
//! its in-flight-guarded recursive core, split out of check.rs. The
//! in-flight set names every generic instantiation currently being
//! expanded so a recursive `type Rec<T> = { next: Rec<T> | null }`
//! closes its back-edge as nominal `Type::ClassRef("Rec<number>")`
//! instead of recursing forever (V3-05 placeholder scheme mirror).

use std::collections::HashMap;

use crate::check::{GenericAliasMap, Type};
use crate::check_type_ann_substitute::ann_substitute;

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
    // V3-18 wedge — `Array<T>` / `ReadonlyArray<T>` / `Iterable<T>`
    // generic shorthand for `T[]`. TS users write both interchangeably
    // and the spec treats `Array<T>` as the canonical Library form;
    // tora's type-ann parser already produces the `Array<T>` flat
    // string but had no mapping. ReadonlyArray is identical semantically
    // (the immutability marker has no runtime effect in the subset).
    // Iterable<T> resolves to Array<T> for typecheck purposes (the
    // for-of source path treats arrays as iterables).
    if let Some(open_idx) = name.find('<')
        && name.ends_with('>')
        && !name.starts_with("__fn(")
        && !name.starts_with("__cls(")
        && !name.starts_with("__env(")
    {
        let head = &name[..open_idx];
        if matches!(head, "Array" | "ReadonlyArray" | "Iterable") {
            let inner = &name[open_idx + 1..name.len() - 1];
            // Single arg only — Array<T1, T2> is invalid TS.
            if !inner.contains('|') {
                return resolve_type_ann_inner(
                    inner,
                    aliases,
                    type_params,
                    generic_aliases,
                    in_flight,
                )
                .map(|t| Type::Array(Box::new(t)));
            }
        }
        // P5.1 — `IteratorResult<T>` is the spec-shaped step value
        // produced by an iterator's `next()` method (ES §27.1.2.1).
        // Structural alias for `{ value: T, done: boolean }`. The
        // existing generator desugar emits the same shape under a
        // per-generator `__step_<name>` alias; this lets user-class
        // iterators (P5.2) annotate `next(): IteratorResult<T>`
        // without minting their own per-iterator alias.
        if head == "IteratorResult" {
            let inner = &name[open_idx + 1..name.len() - 1];
            if !inner.contains('|')
                && let Some(value_ty) =
                    resolve_type_ann_inner(inner, aliases, type_params, generic_aliases, in_flight)
            {
                return Some(Type::Struct(vec![
                    ("value".into(), value_ty),
                    ("done".into(), Type::Boolean),
                ]));
            }
        }
        // P5.1 — `Iterator<T>` / `IterableIterator<T>` are opaque
        // iterable objects whose only typed surface is `.next() →
        // IteratorResult<T>`. Resolved as Type::Any for now — the
        // for-of dispatch in P5.3 Phase B will inspect the runtime
        // class to find the `[Symbol.iterator]` / `next` methods.
        // User can still annotate fields as `Iterator<T>` without
        // surfacing a "unresolved type" error.
        if matches!(head, "Iterator" | "IterableIterator") {
            let inner = &name[open_idx + 1..name.len() - 1];
            if !inner.contains('|') {
                let _ =
                    resolve_type_ann_inner(inner, aliases, type_params, generic_aliases, in_flight);
                return Some(Type::Any);
            }
        }
    }
    // M3.4 — generic struct instantiation: `Foo<arg1|arg2|...>`. Same
    // depth-aware decoder as `__fn(...)`. Substitutes type-args into the
    // original decl's field annotations (as strings), then recursively
    // resolves each substituted field type.
    if let Some(open_idx) = name.find('<')
        && name.ends_with('>')
        && !name.starts_with("__fn(")
        && !name.starts_with("__cls(")
        && !name.starts_with("__env(")
    {
        let head = &name[..open_idx];
        if let Some((tp_names, fields)) = generic_aliases.get(head) {
            let inner = &name[open_idx + 1..name.len() - 1];
            // Split inner at depth-0 `|`.
            let mut args: Vec<&str> = Vec::new();
            let mut depth: i32 = 0;
            let mut last = 0usize;
            let bytes = inner.as_bytes();
            for (i, &b) in bytes.iter().enumerate() {
                match b {
                    b'<' | b'(' => depth += 1,
                    b'>' | b')' => depth -= 1,
                    b'|' if depth == 0 => {
                        args.push(&inner[last..i]);
                        last = i + 1;
                    }
                    _ => {}
                }
            }
            if !inner.is_empty() {
                args.push(&inner[last..]);
            }
            if args.len() != tp_names.len() {
                return None;
            }
            // Substitute each tp_name with its arg ann in every field's
            // annotation string, then recursively resolve.
            let subst: Vec<(String, String)> = tp_names
                .iter()
                .cloned()
                .zip(args.iter().map(|s| s.to_string()))
                .collect();
            // V3-18 wedge — generic bare alias (`type Pair<T> = T[]`)
            // uses the same single-field "__alias__" sentinel; resolve
            // directly to the substituted underlying type instead of
            // wrapping in a Struct.
            if fields.len() == 1 && fields[0].0 == "__alias__" {
                let substituted = ann_substitute(&fields[0].1, &subst);
                return resolve_type_ann_inner(
                    &substituted,
                    aliases,
                    type_params,
                    generic_aliases,
                    in_flight,
                );
            }
            // Recursive instantiation (`type Rec<T> = { next: Rec<T> |
            // null }`): a field of `Rec<number>` mentions `Rec<number>`
            // again. Without a guard this recurses forever — Struct is
            // a structural tree and a recursive type has no finite
            // expansion. Mirror of the non-generic V3-05 solution: the
            // back-edge becomes a nominal ClassRef whose name is the
            // full instantiation key (its own canonical ann), resolved
            // lazily one layer per access by resolve_class_ref.
            if in_flight.contains(name) {
                return Some(Type::ClassRef(name.to_string()));
            }
            in_flight.insert(name.to_string());
            let mut field_tys: Vec<(String, Type)> = Vec::new();
            for (fname, fann) in fields {
                let substituted = ann_substitute(fann, &subst);
                let Some(ty) = resolve_type_ann_inner(
                    &substituted,
                    aliases,
                    type_params,
                    generic_aliases,
                    in_flight,
                ) else {
                    in_flight.remove(name);
                    return None;
                };
                field_tys.push((fname.clone(), ty));
            }
            in_flight.remove(name);
            return Some(Type::Struct(field_tys));
        }
        // T-15 (v0.5.0) — `Promise<T>` is a built-in generic type
        // when the user hasn't shadowed it with a `class Promise<T>`
        // (which would have populated generic_aliases above). The
        // resolver checks user decls FIRST so existing user-class
        // patterns (e.g. test262-port/promise-001-basic.ts) keep
        // working through the v0.5 transition. T-15.h reserves
        // `Promise` as a built-in name and forces migration.
        if head == "Promise" {
            let inner = &name[head.len() + 1..name.len() - 1];
            let inner_ty =
                resolve_type_ann_inner(inner, aliases, type_params, generic_aliases, in_flight)?;
            return Some(Type::Promise(Box::new(inner_ty)));
        }
        // `Map<K, V>` / `Set<T>` / `WeakMap<K, V>` / `WeakSet<T>` —
        // builtin container generics whose K / V are erased at the
        // type-system layer (Type::Map / Type::Set / Type::WeakMap /
        // Type::WeakSet have no Box payload — typed `.get` / `.set`
        // arg/return wiring is handled elsewhere via the method-table
        // arm in check.rs). Validate each type-arg is resolvable so
        // typos still surface, then return the erased container type.
        // Mirrors the bare `Map` / `Set` arm below; this arm covers the
        // parametric spelling users actually write (`Map<number, string>`
        // becomes the flat ann string `Map<number|string>` from
        // `parse_type_ann`).
        if matches!(
            head,
            "Map"
                | "Set"
                | "WeakMap"
                | "WeakSet"
                | "ReadonlyMap"
                | "ReadonlySet"
                | "MapIter"
                | "mapiter"
                | "ArrIter"
                | "arriter"
        ) {
            let inner = &name[open_idx + 1..name.len() - 1];
            // Depth-aware split of inner at `|` (same shape as the
            // generic-instantiation decoder above) so nested generics
            // like `Map<string, Array<number>>` (flat `Map<string|Array<number>>`)
            // resolve their args correctly.
            let bytes = inner.as_bytes();
            let mut depth: i32 = 0;
            let mut last = 0usize;
            let mut args: Vec<&str> = Vec::new();
            for (i, &b) in bytes.iter().enumerate() {
                match b {
                    b'<' | b'(' => depth += 1,
                    b'>' | b')' => depth -= 1,
                    b'|' if depth == 0 => {
                        args.push(&inner[last..i]);
                        last = i + 1;
                    }
                    _ => {}
                }
            }
            if !inner.is_empty() {
                args.push(&inner[last..]);
            }
            for arg in &args {
                resolve_type_ann_inner(arg, aliases, type_params, generic_aliases, in_flight)?;
            }
            return Some(match head {
                "Map" | "ReadonlyMap" => Type::Map,
                "Set" | "ReadonlySet" => Type::Set,
                "WeakMap" => Type::WeakMap,
                "WeakSet" => Type::WeakSet,
                "MapIter" | "mapiter" => Type::MapIter,
                "ArrIter" | "arriter" => Type::ArrIter,
                _ => unreachable!(),
            });
        }
        return None;
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
    // M2 Phase B Stage 1 — fn type annotations encoded as
    // V3-18 P2.4.c.2 — inline obj type `__inlobj(name1:T1|name2:T2|...)`.
    // Same depth-aware decoder shape as `__fn(...)`. Each field's type
    // recurses through resolve_type_ann_full so nested inline obj /
    // generic types work.
    if let Some(rest) = name.strip_prefix("__inlobj(") {
        let bytes = rest.as_bytes();
        let mut depth: i32 = 1;
        let mut close_idx = None;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close_idx?;
        let fields_str = &rest[..close];
        let mut fields_split: Vec<&str> = Vec::new();
        let mut depth2: i32 = 0;
        let mut last = 0usize;
        for (i, &b) in fields_str.as_bytes().iter().enumerate() {
            match b {
                b'(' => depth2 += 1,
                b')' => depth2 -= 1,
                b'|' if depth2 == 0 => {
                    fields_split.push(&fields_str[last..i]);
                    last = i + 1;
                }
                _ => {}
            }
        }
        if !fields_str.is_empty() {
            fields_split.push(&fields_str[last..]);
        }
        let mut fields_out: Vec<(String, Type)> = Vec::with_capacity(fields_split.len());
        for f in fields_split {
            let colon = f.find(':')?;
            let fname = f[..colon].to_string();
            let fty_str = &f[colon + 1..];
            let fty =
                resolve_type_ann_inner(fty_str, aliases, type_params, generic_aliases, in_flight)?;
            fields_out.push((fname, fty));
        }
        return Some(Type::Struct(fields_out));
    }
    // `__fn(P1|P2|...)->R` (user-source fn type) and its
    // `tag_struct_field_closure_types`-tagged sibling `__cls(P1|...)->R`
    // (struct-field closure slot) share the same parse shape and both
    // resolve to `Type::Function(params, ret)` at the typecheck layer.
    // SSA `parse_type` is what actually distinguishes them: `__fn` →
    // `Type::FnSig` (direct dispatch), `__cls` → `Type::Closure`
    // (env-first dispatch).
    if let Some(rest) = name
        .strip_prefix("__fn(")
        .or_else(|| name.strip_prefix("__cls("))
    {
        let bytes = rest.as_bytes();
        let mut depth: i32 = 1;
        let mut close_idx = None;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close_idx?;
        let params_str = &rest[..close];
        let after = &rest[close + 1..];
        let ret_str = after.strip_prefix("->")?;

        let mut params = Vec::new();
        let mut depth2: i32 = 0;
        let mut last = 0usize;
        for (i, &b) in params_str.as_bytes().iter().enumerate() {
            match b {
                b'(' => depth2 += 1,
                b')' => depth2 -= 1,
                b'|' if depth2 == 0 => {
                    params.push(&params_str[last..i]);
                    last = i + 1;
                }
                _ => {}
            }
        }
        if !params_str.is_empty() {
            params.push(&params_str[last..]);
        }

        let mut param_tys = Vec::with_capacity(params.len());
        for p in params {
            param_tys.push(resolve_type_ann_inner(
                p,
                aliases,
                type_params,
                generic_aliases,
                in_flight,
            )?);
        }
        let ret_ty =
            resolve_type_ann_inner(ret_str, aliases, type_params, generic_aliases, in_flight)?;
        return Some(Type::Function(param_tys, Box::new(ret_ty)));
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
