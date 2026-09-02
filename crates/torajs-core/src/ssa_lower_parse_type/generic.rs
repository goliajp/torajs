//! Generic-annotation resolution (`Head<args>` spellings) of
//! [`super::parse_type`], split out 2026-07-03 (fn-debt decomp).
//! Bodies verbatim from the pre-split guard block; `None` = no arm
//! matched, caller falls through to the primitive match (mirror of
//! `check_type_ann/generic.rs` on the checker side).

use crate::ast::PropKey;
use std::collections::HashMap;

use crate::ssa::{self, Type};
use crate::ssa_lower::substitute_in_ann;

use super::{parse_struct_field_type, parse_type};

#[allow(clippy::too_many_arguments)]
pub(super) fn parse_generic(
    s: &str,
    open_idx: usize,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(PropKey, String)>)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Option<Type> {
    let head = &s[..open_idx];
    if let Some((tp_names, fields)) = generic_struct_decls.get(head).cloned() {
        let inner = &s[open_idx + 1..s.len() - 1];
        let mut args: Vec<&str> = Vec::new();
        let mut depth: i32 = 0;
        let mut last = 0usize;
        let mut prev: u8 = 0;
        for (i, &b) in inner.as_bytes().iter().enumerate() {
            match b {
                b'<' | b'(' => depth += 1,
                // Chunk 795 — the `>` of a fn-type return arrow
                // (`Pair<__fn()->(number)|string>`) is not a generic
                // closer; counting it dropped the depth below zero
                // and the depth-0 `|` between the type args went
                // unseen (chunk-794 splitter-family mirror).
                b'>' if prev == b'-' => {}
                b'>' | b')' => depth -= 1,
                b'|' if depth == 0 => {
                    args.push(&inner[last..i]);
                    last = i + 1;
                }
                _ => {}
            }
            prev = b;
        }
        if !inner.is_empty() {
            args.push(&inner[last..]);
        }
        if args.len() != tp_names.len() {
            panic!(
                "ssa-lower: generic struct `{head}` expects {} type args, got {}",
                tp_names.len(),
                args.len()
            );
        }
        let subst: Vec<(String, String)> = tp_names
            .iter()
            .cloned()
            .zip(args.iter().map(|a| a.to_string()))
            .collect();
        // V3-18 wedge — generic bare alias (`type Pair<T> = T[]`)
        // uses single-field "__alias__" sentinel; resolve to the
        // substituted underlying type instead of synthesizing a
        // struct.
        if fields.len() == 1 && fields[0].0 == "__alias__" {
            let substituted = substitute_in_ann(&fields[0].1, &subst);
            return Some(parse_type(
                Some(&substituted),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            ));
        }
        // Reserve-first with a persistent memo (mirror of the
        // non-generic V3-05 phase-1 reserved-sid scheme, and of
        // the checker's in-flight ClassRef back-edge). A recursive
        // alias (`type Rec<T> = { next: Rec<T> | null }`) mentions
        // its own key while its fields are being parsed — the memo
        // hit closes that back-edge on the reserved sid instead of
        // recursing forever. The memo lives for the whole lower()
        // so every mention of one key shares one nominal sid
        // (instantiations are no longer structurally interned —
        // nominal identity per key, rustc-style).
        if let Some(sid) = inst_memo.get(s) {
            return Some(Type::Obj(*sid));
        }
        let id = ssa::StructId(struct_layouts.len() as u32);
        struct_layouts.push(Vec::new());
        inst_memo.insert(s.to_string(), id);
        let mut layout: Vec<(PropKey, Type)> = Vec::with_capacity(fields.len());
        for (fname, fann) in &fields {
            let substituted = substitute_in_ann(fann, &subst);
            // Chunk 795 — struct-field fn slots are Closure-repr.
            // `tag_struct_field_closure_types` retags TypeDecl field
            // anns, but a generic decl spells the field as its type
            // param ("v: T") — the `__fn(` only appears here after
            // substitution, so retag at the instantiation point
            // (mirror of the chunk-793 inline-object birth-site
            // retag; without it `Box<() => number>` held a FnSig
            // layout slot while the literal stored a closure env
            // block — SIGBUS on the field call).
            let substituted = if let Some(rest) = substituted.strip_prefix("__fn(") {
                format!("__cls({rest}")
            } else if let Some(rest) = substituted.strip_prefix("__nullable(__fn(") {
                format!("__nullable(__cls({rest}")
            } else {
                substituted
            };
            let fty = parse_struct_field_type(
                &substituted,
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            );
            layout.push((fname.clone(), fty));
        }
        struct_layouts[id.0 as usize] = layout;
        return Some(Type::Obj(id));
    }
    // T-15.f.2 — `Promise<T>` builtin generic. Inner T type-erased
    // at SSA (mirror of `check.rs::resolve_type_ann_full`).
    if head == "Promise" {
        return Some(Type::Promise);
    }
    // V3-18 wedge — `Array<T>` / `ReadonlyArray<T>` / `Iterable<T>`
    // generic shorthand for `T[]`. Mirror of the resolver in
    // check.rs::resolve_type_ann_full so SSA + check agree.
    if matches!(head, "Array" | "ReadonlyArray" | "Iterable") {
        let inner = &s[open_idx + 1..s.len() - 1];
        if !inner.contains('|') {
            let elem_str = format!("{inner}[]");
            return Some(parse_type(
                Some(&elem_str),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            ));
        }
    }
    // P5.1 — `IteratorResult<T>` → `{ value: T, done: boolean }`
    // struct (layout reuses `__step_<gen>` generator shape).
    if head == "IteratorResult" {
        let inner = &s[open_idx + 1..s.len() - 1];
        if !inner.contains('|') {
            let inlobj = format!("__inlobj(value:{inner}|done:boolean)");
            return Some(parse_type(
                Some(&inlobj),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            ));
        }
    }
    // P5.1 — `Iterator<T>` / `IterableIterator<T>` opaque at SSA
    // (Any-tier slot; P5.3 Phase B dispatches via class runtime).
    if matches!(head, "Iterator" | "IterableIterator") {
        return Some(Type::Any);
    }
    // `Map<K,V>` / `Set<T>` / `WeakMap<K,V>` / `WeakSet<T>` — K/V
    // erased at SSA (mirror of `check_type_ann.rs`).
    if let Some(t) = match head {
        "Map" | "ReadonlyMap" => Some(Type::Map),
        "Set" | "ReadonlySet" => Some(Type::Set),
        "WeakMap" => Some(Type::WeakMap),
        "WeakSet" => Some(Type::WeakSet),
        // Chunk 629 — `WeakRef<T>` erases like its weak-family
        // siblings (mirror of check_type_ann's generic arm).
        "WeakRef" => Some(Type::WeakRef),
        // P6.4b/c generic-ann form `MapIter<T>` / `ArrIter<T>` —
        // the type-arg is erased at SSA the same way Map/Set
        // generics erase to Type::Map / Type::Set above.
        "MapIter" | "mapiter" => Some(Type::MapIter),
        "ArrIter" | "arriter" => Some(Type::ArrIter),
        _ => None,
    } {
        return Some(t);
    }
    None
}
