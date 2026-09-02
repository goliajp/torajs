//! Generic-annotation resolution (`Head<args>` spellings), split out
//! of `resolve_type_ann_inner` (2026-07-03, fn-debt decomp). The two
//! pre-split guard blocks (builtin generics that fall through, then
//! generic-alias instantiation / Promise / erased containers ending
//! in a hard None) merge into one fn — the caller's single `<...>`
//! guard reaches this in the same order, so fall-through semantics
//! are unchanged. The two inline depth-aware `|` splitters collapse
//! into `markers::split_top_pipe`.

use crate::ast::PropKey;
use std::collections::{HashMap, HashSet};

use crate::check::{GenericAliasMap, Type};
use crate::check_type_ann_substitute::ann_substitute;

use super::markers::split_top_pipe;
use super::resolve_type_ann_inner;

pub(super) fn resolve_generic(
    name: &str,
    open_idx: usize,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
    in_flight: &mut HashSet<String>,
) -> Option<Type> {
    let head = &name[..open_idx];
    if matches!(head, "Array" | "ReadonlyArray" | "Iterable") {
        let inner = &name[open_idx + 1..name.len() - 1];
        // Single arg only — Array<T1, T2> is invalid TS.
        if !inner.contains('|') {
            return resolve_type_ann_inner(inner, aliases, type_params, generic_aliases, in_flight)
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
            let _ = resolve_type_ann_inner(inner, aliases, type_params, generic_aliases, in_flight);
            return Some(Type::Any);
        }
    }
    if let Some((_, _, is_class)) = generic_aliases.get(head) {
        // Blade 3 (RFC 20260815-generic-nominal-identity) — a generic
        // CLASS instantiation is nominal: `Box<number>` answers
        // `ClassRef("Box<number>")` (the full key is its own canonical
        // ann), and structural consumers force one layer through
        // `resolve_class_ref` → `expand_instantiation_full` exactly
        // like the recursive-alias back-edge. A generic `type` alias
        // keeps the structural expansion — it has no nominal identity.
        if *is_class {
            return Some(Type::ClassRef(name.to_string()));
        }
        return expand_instantiation(
            name,
            open_idx,
            aliases,
            type_params,
            generic_aliases,
            in_flight,
        );
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
            | "WeakRef"
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
        let args = split_top_pipe(inner);
        for arg in &args {
            resolve_type_ann_inner(arg, aliases, type_params, generic_aliases, in_flight)?;
        }
        return Some(match head {
            "Map" | "ReadonlyMap" => Type::Map,
            "Set" | "ReadonlySet" => Type::Set,
            "WeakMap" => Type::WeakMap,
            "WeakSet" => Type::WeakSet,
            // Chunk 629 — `WeakRef<T>` erases like its weak-family
            // siblings (deref() answers Nullable<Any>; the target
            // type is validated above, then dropped).
            "WeakRef" => Type::WeakRef,
            "MapIter" | "mapiter" => Type::MapIter,
            "ArrIter" | "arriter" => Type::ArrIter,
            _ => unreachable!(),
        });
    }
    None
}

/// The user generic-alias / generic-class instantiation body — the
/// `generic_aliases` hit of [`resolve_generic`], verbatim. Split out
/// (rotation 413 blade 3a) so `resolve_class_ref` can FORCE one layer
/// of structural expansion for a `ClassRef("Box<number>")` key without
/// going through `resolve_generic` — the nominal short-circuit blade 3
/// adds there would otherwise answer the same ClassRef back and the
/// unwrap could never make progress.
fn expand_instantiation(
    name: &str,
    open_idx: usize,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
    in_flight: &mut HashSet<String>,
) -> Option<Type> {
    let head = &name[..open_idx];
    let (tp_names, fields, _) = generic_aliases.get(head)?;
    let inner = &name[open_idx + 1..name.len() - 1];
    // Split inner at depth-0 `|`.
    let args = split_top_pipe(inner);
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
    let mut field_tys: Vec<(PropKey, Type)> = Vec::new();
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
        field_tys.push((PropKey::from(fname), ty));
    }
    in_flight.remove(name);
    Some(Type::Struct(field_tys))
}

/// Force-expand entry for a generic-instantiation ClassRef key
/// (`"Box<number>"`) — one structural layer, bypassing the nominal
/// short-circuit `resolve_generic` applies to generic classes. Falls
/// back to the ordinary resolver for any other key shape, which keeps
/// it exactly equivalent to the `resolve_type_ann_full` call it
/// replaced at the `resolve_class_ref` unwrap site. `type_params`
/// names the tp environment for keys spelled inside a generic body
/// (`"Box<T>"` — the args must resolve as TypeVars there); an
/// instantiated key passes `&[]`.
pub(crate) fn expand_instantiation_full(
    name: &str,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
) -> Option<Type> {
    if let Some(open_idx) = name.find('<')
        && name.ends_with('>')
        && generic_aliases.contains_key(&name[..open_idx])
    {
        let mut in_flight = HashSet::new();
        return expand_instantiation(
            name,
            open_idx,
            aliases,
            type_params,
            generic_aliases,
            &mut in_flight,
        );
    }
    super::resolve_type_ann_full(name, aliases, type_params, generic_aliases)
}
