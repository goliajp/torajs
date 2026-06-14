//! Type-annotation string → `ssa::Type` resolution (`parse_type`),
//! split out of ssa_lower.rs (file-size known-debt: ssa_lower.rs only
//! shrinks). Generic struct instantiations resolve through a
//! reserve-first persistent memo (`inst_memo`) so recursive aliases
//! (`type Rec<T> = { next: Rec<T> | null }`) close their back-edge on
//! the reserved nominal sid — the lower-layer mirror of the checker's
//! in-flight ClassRef scheme (see rfcs/20260612-generic-recursive-alias).

use std::collections::HashMap;

use crate::ssa::{self, Type};
use crate::ssa_lower::{intern_arr_layout, intern_fn_sig, substitute_in_ann};

pub(crate) fn parse_type(
    ann: Option<&str>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Type {
    let s = match ann {
        Some(s) => s,
        None => return Type::Void,
    };
    // `__nullable(T)` — at SSA storage / ABI level, identical to T.
    // The `null` value is just an in-band 0 sentinel for pointer-shaped
    // T. check.rs is the only layer that distinguishes T from
    // Nullable(T); by here it's already enforced the rules.
    if let Some(rest) = s.strip_prefix("__nullable(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return parse_type(
            Some(inner),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
    }
    if s == "null" {
        // Bare `null` annotation (rare). Pointer-shaped, value is null.
        return Type::Ptr;
    }
    // `T[]` array suffix. Recurse on the element type, intern, return Arr.
    // The flat string is produced by parser::parse_type_ann, so we can
    // strip a trailing "[]" and recurse cleanly. Multi-dim arrays
    // (`T[][]`) work via the recursion: `number[][]` → strip to
    // `number[]` → strip to `number` → I64; intern outer-to-inner.
    if let Some(rest) = s.strip_suffix("[]") {
        let elem = parse_type(
            Some(rest),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
        let id = intern_arr_layout(arr_layouts, elem);
        return Type::Arr(id);
    }
    // M2 — closure env marker `__env(cap0|cap1|...)` injected by
    // `lift_arrow_fns` on the hidden first param of capturing arrows. At
    // SSA the env is just an opaque pointer; the capture names are
    // re-decoded by `lower_fn` below to emit the env-load preamble.
    if s.starts_with("__env(") && s.ends_with(')') {
        return Type::Ptr;
    }
    // M3 fix — structural struct annotation `__struct(name:T|...)`,
    // produced by `check::type_to_ann` for monomorphized generics that
    // bind a struct type. Decode each field, intern the layout, return
    // `Type::Obj(StructId)`. Same depth-aware split as `__fn(...)`.
    // V3-18 P2.4.c.2 — `__inlobj(...)` alias for inline object type
    // literals from the parser. Same shape as `__struct(...)`; defer
    // by rewriting the prefix.
    if s.starts_with("__inlobj(") && s.ends_with(')') {
        let rewritten = format!("__struct({}", &s[9..]);
        return parse_type(
            Some(&rewritten),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
    }
    if let Some(rest) = s.strip_prefix("__struct(")
        && s.ends_with(')')
    {
        let inner = &rest[..rest.len() - 1];
        let mut fields: Vec<(String, Type)> = Vec::new();
        let mut depth: i32 = 0;
        let mut last = 0usize;
        let bytes = inner.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' | b'<' => depth += 1,
                b')' | b'>' => depth -= 1,
                b'|' if depth == 0 => {
                    let part = &inner[last..i];
                    let (n, t) = part.split_once(':').unwrap_or((part, ""));
                    let fty = parse_type(
                        Some(t),
                        aliases,
                        arr_layouts,
                        fn_sigs,
                        generic_struct_decls,
                        struct_layouts,
                        inst_memo,
                    );
                    fields.push((n.to_string(), fty));
                    last = i + 1;
                }
                _ => {}
            }
        }
        if !inner.is_empty() {
            let part = &inner[last..];
            let (n, t) = part.split_once(':').unwrap_or((part, ""));
            let fty = parse_type(
                Some(t),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            );
            fields.push((n.to_string(), fty));
        }
        // Intern by structural equality.
        for (i, ex) in struct_layouts.iter().enumerate() {
            if *ex == fields {
                return Type::Obj(ssa::StructId(i as u32));
            }
        }
        let id = ssa::StructId(struct_layouts.len() as u32);
        struct_layouts.push(fields);
        return Type::Obj(id);
    }
    // M2 Phase B Stage 2 — fn type `__fn(P1|P2|...)->R`. Same encoding
    // produced by parser::parse_type_ann; same depth-aware decoding as
    // check.rs's resolve_type_ann (so SSA + check agree on the signature
    // structure).
    if let Some(rest) = s.strip_prefix("__fn(") {
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
        let close = close_idx.unwrap_or_else(|| panic!("ssa-lower: malformed fn-type `{s}`"));
        let params_str = &rest[..close];
        let after = &rest[close + 1..];
        let ret_str = after
            .strip_prefix("->")
            .unwrap_or_else(|| panic!("ssa-lower: malformed fn-type ret `{s}`"));

        // Split params at depth-0 `|`.
        let mut params: Vec<Type> = Vec::new();
        let mut depth2: i32 = 0;
        let mut last = 0usize;
        let pb = params_str.as_bytes();
        for (i, &b) in pb.iter().enumerate() {
            match b {
                b'(' => depth2 += 1,
                b')' => depth2 -= 1,
                b'|' if depth2 == 0 => {
                    params.push(parse_type(
                        Some(&params_str[last..i]),
                        aliases,
                        arr_layouts,
                        fn_sigs,
                        generic_struct_decls,
                        struct_layouts,
                        inst_memo,
                    ));
                    last = i + 1;
                }
                _ => {}
            }
        }
        if !params_str.is_empty() {
            params.push(parse_type(
                Some(&params_str[last..]),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            ));
        }
        let ret = parse_type(
            Some(ret_str),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
        let id = intern_fn_sig(fn_sigs, params, ret);
        return Type::FnSig(id);
    }
    // P3.closure-in-struct-field — TypeDecl field types tagged with
    // `__cls(P)->R` by the `tag_struct_field_closure_types` desugar
    // pass. This is the narrow set of `(...)=>R` annotations that
    // actually need the Closure (env-first CallIndirect) ABI: struct
    // fields can store both FnSig (top-level FnDecl ref, wrapped by
    // `synthesize_fn_to_closure_forwarders` ObjectLit arm) and Closure
    // values (capturing function expressions lifted by
    // `lift_arrow_fns`), so the slot has to be Closure-typed and the
    // forwarder pass wraps any FnSig store-site at construction time.
    //
    // Fn-typed param / return / let bindings keep `__fn(P)->R` →
    // Type::FnSig, preserving direct (non-env-first) dispatch on the
    // hot fn-as-callback path (`reduce(xs, add1)` etc.).
    if let Some(rest) = s.strip_prefix("__cls(") {
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
        let close = close_idx.unwrap_or_else(|| panic!("ssa-lower: malformed cls-type `{s}`"));
        let params_str = &rest[..close];
        let after = &rest[close + 1..];
        let ret_str = after
            .strip_prefix("->")
            .unwrap_or_else(|| panic!("ssa-lower: malformed cls-type ret `{s}`"));
        let mut params: Vec<Type> = Vec::new();
        let mut depth2: i32 = 0;
        let mut last = 0usize;
        let pb = params_str.as_bytes();
        for (i, &b) in pb.iter().enumerate() {
            match b {
                b'(' => depth2 += 1,
                b')' => depth2 -= 1,
                b'|' if depth2 == 0 => {
                    params.push(parse_type(
                        Some(&params_str[last..i]),
                        aliases,
                        arr_layouts,
                        fn_sigs,
                        generic_struct_decls,
                        struct_layouts,
                        inst_memo,
                    ));
                    last = i + 1;
                }
                _ => {}
            }
        }
        if !params_str.is_empty() {
            params.push(parse_type(
                Some(&params_str[last..]),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            ));
        }
        let ret = parse_type(
            Some(ret_str),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
        let id = intern_fn_sig(fn_sigs, params, ret);
        return Type::Closure(id);
    }
    // M3.4 — generic struct instantiation `Foo<arg1|arg2|...>`. Same
    // depth-aware split as `__fn(...)`. Substitute type-params into each
    // field annotation (string-level word-boundary substitution) and
    // recursively parse to get field types, then intern the layout into
    // module.struct_layouts.
    if let Some(open_idx) = s.find('<')
        && s.ends_with('>')
    {
        let head = &s[..open_idx];
        if let Some((tp_names, fields)) = generic_struct_decls.get(head).cloned() {
            let inner = &s[open_idx + 1..s.len() - 1];
            let mut args: Vec<&str> = Vec::new();
            let mut depth: i32 = 0;
            let mut last = 0usize;
            for (i, &b) in inner.as_bytes().iter().enumerate() {
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
                return parse_type(
                    Some(&substituted),
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                );
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
                return Type::Obj(*sid);
            }
            let id = ssa::StructId(struct_layouts.len() as u32);
            struct_layouts.push(Vec::new());
            inst_memo.insert(s.to_string(), id);
            let mut layout: Vec<(String, Type)> = Vec::with_capacity(fields.len());
            for (fname, fann) in &fields {
                let substituted = substitute_in_ann(fann, &subst);
                let fty = parse_type(
                    Some(&substituted),
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
            return Type::Obj(id);
        }
        // T-15.f.2 — `Promise<T>` builtin generic. Inner T type-erased
        // at SSA (mirror of `check.rs::resolve_type_ann_full`).
        if head == "Promise" {
            return Type::Promise;
        }
        // V3-18 wedge — `Array<T>` / `ReadonlyArray<T>` / `Iterable<T>`
        // generic shorthand for `T[]`. Mirror of the resolver in
        // check.rs::resolve_type_ann_full so SSA + check agree.
        if matches!(head, "Array" | "ReadonlyArray" | "Iterable") {
            let inner = &s[open_idx + 1..s.len() - 1];
            if !inner.contains('|') {
                let elem_str = format!("{inner}[]");
                return parse_type(
                    Some(&elem_str),
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                );
            }
        }
        // P5.1 — `IteratorResult<T>` → `{ value: T, done: boolean }`
        // struct (layout reuses `__step_<gen>` generator shape).
        if head == "IteratorResult" {
            let inner = &s[open_idx + 1..s.len() - 1];
            if !inner.contains('|') {
                let inlobj = format!("__inlobj(value:{inner}|done:boolean)");
                return parse_type(
                    Some(&inlobj),
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                    inst_memo,
                );
            }
        }
        // P5.1 — `Iterator<T>` / `IterableIterator<T>` opaque at SSA
        // (Any-tier slot; P5.3 Phase B dispatches via class runtime).
        if matches!(head, "Iterator" | "IterableIterator") {
            return Type::Any;
        }
        // `Map<K,V>` / `Set<T>` / `WeakMap<K,V>` / `WeakSet<T>` — K/V
        // erased at SSA (mirror of `check_type_ann.rs`).
        if let Some(t) = match head {
            "Map" | "ReadonlyMap" => Some(Type::Map),
            "Set" | "ReadonlySet" => Some(Type::Set),
            "WeakMap" => Some(Type::WeakMap),
            "WeakSet" => Some(Type::WeakSet),
            _ => None,
        } {
            return t;
        }
    }
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
        "regex" | "RegExp" => Type::RegExp,
        "date" => Type::Date,
        // T-21 — `fetch(url)` Response heap struct; ptr at SSA.
        "Response" => Type::Ptr,
        // T-10.a — Any plumbing; single 64B ptr slot, tag via heap header.
        "any" => Type::Any,
        // TS `object` collapses to Type::Any (non-primitive constraint
        // is independent substrate; mirror of `check_type_ann.rs`).
        "object" => Type::Any,
        // T-13.a (v0.4.0) — Symbol value. Heap-allocated 16-byte
        // block, identity is pointer identity. Lowers to ptr.
        "symbol" => Type::Symbol,
        // T-25 (v0.7) — BigInt value. Heap-allocated sign-magnitude
        // struct (runtime_bigint.c). Lowers to ptr.
        "bigint" => Type::BigInt,
        // T-26 (v0.7) — WeakRef. Heap-allocated 16-byte struct.
        // Type ann is `weakref` (lowercase) since `WeakRef<T>` ann
        // form isn't parsed at SSA layer yet — type-erased.
        "weakref" => Type::WeakRef,
        // P6.1 — strong-ref Map / Set. Type-erased keys + values via
        // the tagged-Any slot layout in runtime_map.c.
        "Map" | "map" => Type::Map,
        "Set" | "set" => Type::Set,
        // P6.4b — Map iterator handle returned by m.keys / .values / .entries.
        "mapiter" => Type::MapIter,
        // P6.4c-C3 — Array<Any> iterator handle.
        "arriter" => Type::ArrIter,
        // T-26.B (v0.7) — WeakMap / WeakSet. Type-erased keys + values.
        "weakmap" => Type::WeakMap,
        "weakset" => Type::WeakSet,
        other => match aliases.get(other) {
            Some(ty) => *ty,
            None => panic!("ssa-lower: unsupported type annotation `{other}`"),
        },
    }
}
