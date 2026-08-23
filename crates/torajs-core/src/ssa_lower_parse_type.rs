//! Type-annotation string → `ssa::Type` resolution (`parse_type`),
//! split out of ssa_lower.rs. Generic struct instantiations resolve
//! through a reserve-first persistent memo (`inst_memo`) so recursive
//! aliases like `type Rec<T> = { next: Rec<T> | null }` close their
//! back-edge on the reserved nominal sid — see
//! rfcs/20260612-generic-recursive-alias.

use std::collections::HashMap;

mod generic;
mod markers;

use crate::ssa::{self, Type};
use crate::ssa_lower::intern_arr_layout;

/// RFC 20260710-optional-undefined-repr C4 — struct FIELD-position
/// type resolution. A `__nullable(number|boolean)` field slot
/// materializes as `Type::Any` (8B NaN-box): scalar slots have no
/// in-band undefined/null encoding, so the optional slot pays the
/// box tax (undefined = ANY_UNDEF, null = ANY_NULL, values box as
/// i64/f64/bool) while non-optional slots stay raw. Param / let /
/// return positions keep the plain [`parse_type`] strip — their ABI
/// is unchanged. A `__nullable(<alias>)` whose alias RESOLVES to a
/// scalar is not covered yet (keeps the pre-RFC in-band collapse);
/// every other spelling delegates verbatim.
/// Does `__nullable(<inner>)` materialize as `Type::Any` rather than
/// as the pointer-shaped in-band collapse?
///
/// A scalar `T | null` has nowhere to PUT the null — the in-band 0
/// sentinel is a legitimate `0` / `false` — so it pays the 8B NaN-box
/// tax instead (RFC 20260710-optional-undefined-repr C4). A
/// pointer-shaped T genuinely has the bit pattern to spare and stays
/// raw. An alias RESOLVING to a scalar is not covered (it keeps the
/// pre-RFC collapse), the same gap the field arm has.
///
/// Every position that has to spell "the absent value" for such a slot
/// asks HERE rather than restating the list — the parser's implicit
/// optional-parameter default is the second caller, and two spellings
/// of one rule is how the answers drift apart.
pub(crate) fn nullable_inner_boxes(inner: &str) -> bool {
    matches!(inner, "number" | "boolean")
}

pub(crate) fn parse_struct_field_type(
    ann: &str,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Type {
    if let Some(rest) = ann.strip_prefix("__nullable(")
        && let Some(inner) = rest.strip_suffix(')')
        && nullable_inner_boxes(inner)
    {
        return Type::Any;
    }
    parse_type(
        Some(ann),
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
    )
}

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
    // A scalar `T | null` has nowhere to PUT the null: the in-band 0
    // sentinel below is a legitimate `0` / `false`, so the slot cannot
    // tell the two apart and every null test on it answers the value's
    // answer — `a === null` is false, `typeof a` is "number", and an
    // `if (a !== null)` guard runs its narrowed branch on a null.
    // RFC 20260710-optional-undefined-repr C4 already settled the
    // remedy for the struct-FIELD position (materialize Any, pay the
    // 8B NaN-box tax so undefined = ANY_UNDEF / null = ANY_NULL and
    // values box); it just never reached the let / param / return
    // positions, which this shares with it verbatim.
    if let Some(rest) = s.strip_prefix("__nullable(")
        && let Some(inner) = rest.strip_suffix(')')
        && nullable_inner_boxes(inner)
    {
        return Type::Any;
    }
    // `__nullable(T)` for a pointer-shaped T — at SSA storage / ABI
    // level, identical to T. The `null` value is an in-band 0
    // sentinel, which a pointer slot genuinely has spare. check.rs is
    // the only layer that distinguishes T from Nullable(T); by here
    // it's already enforced the rules.
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
    if s == "undefined" {
        // The `undefined` type — undefined values lower to
        // `Operand::ConstPtrNull` (Type::Ptr), so an `undefined`-typed
        // slot is the same null-shaped pointer (mirror of `null`).
        return Type::Ptr;
    }
    // `T[]` array suffix — recurse on the element type, intern,
    // return Arr (body in `parse_arr_suffix` below).
    if let Some(rest) = s.strip_suffix("[]") {
        return parse_arr_suffix(
            rest,
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
    }
    // M2 — closure env marker `__env(cap0|cap1|...)` injected by
    // `lift_arrow_fns` on the hidden first param of capturing arrows. At
    // SSA the env is just an opaque pointer; the capture names are
    // re-decoded by `lower_fn` below to emit the env-load preamble.
    if s.starts_with("__env(") && s.ends_with(')') {
        return Type::Ptr;
    }
    // RFC 20260708-closure-argv-face — raw argv pointer marker on
    // the synthetic `__torajs_argv` param (boxed adapter feeds it).
    if s == "__argvptr()" {
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
        return markers::parse_struct(
            inner,
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
    }
    // TS `Function` — the top callable type. Mirrors the checker's
    // collapse (check_type_ann): the slot is `Any`, calls ride the
    // any-call runtime dispatch.
    if s == "Function" {
        return Type::Any;
    }
    // Buffer-family class annotations — the checker's collapse
    // (check_type_ann), mirrored: the cells are any-lane only.
    if s == "ArrayBuffer"
        || s == "DataView"
        || crate::ssa_lower_call_typedarray::kind_of_name(s).is_some()
    {
        return Type::Any;
    }
    // Closure-repr marker family — all three spellings decode via
    // markers::parse_cls (env-first CallIndirect ABI):
    // - RFC 20260708-variadic rest-tail fn type
    //   (`__fn(fixed|__rest(E[]))->R`): the boxed dual entry the
    //   variadic call lane dispatches through lives in the closure
    //   env, so the slot can never be a bare fn ptr. parse_cls skips
    //   the `__rest(` segment (the static sig is the fixed prefix;
    //   the call arm routes through `closure_call_variadic`).
    // - P3.closure-in-struct-field `__cls(P)->R`, tagged by the
    //   `tag_struct_field_closure_types` desugar pass: struct fields
    //   can store both FnSig (forwarder-wrapped at construction) and
    //   capturing Closure values, so the slot is Closure-typed.
    //   Fn-typed param / return / let bindings keep `__fn(P)->R` →
    //   Type::FnSig via try_parse_fn_type below, preserving direct
    //   dispatch on the hot fn-as-callback path.
    // - RFC 20260714-objlit-accessor `__mth(P)->R`: an object-literal
    //   method slot. Closure-repr like `__cls(`, and the params are
    //   the USER params only — `objlit_nominal` builds the ann by
    //   filtering `__env` / `__this` out, so `o.m(x)` types at the
    //   source arity on both sides. A receiver-first body announces
    //   itself through the closure cell's `FLAG_CLOSURE_RECV_FIRST`
    //   instead, which is what the runtime dispatcher reads.
    //   (Measured r379: `t()`'s env-first sig is `(ptr, i64)` — no
    //   receiver slot. The earlier note here claimed the sig KEEPS a
    //   leading receiver and pointed at a `LowerCtx::objlit_method_slots`
    //   that does not exist.)
    if let Some(rest) = s
        .strip_prefix("__fn(")
        .filter(|_| s.contains("__rest("))
        .or_else(|| s.strip_prefix("__cls("))
        .or_else(|| s.strip_prefix("__mth("))
    {
        return markers::parse_cls(
            s,
            rest,
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
    }
    // M2 Phase B Stage 2 — fn type `__fn(P1|P2|...)->R` decoder lives
    // in the sibling `ssa_lower_parse_fn_type` module.
    if let Some(ty) = crate::ssa_lower_parse_fn_type::try_parse_fn_type(
        s,
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
    ) {
        return ty;
    }
    // M3.4 — generic struct instantiation `Foo<arg1|arg2|...>`. Same
    // depth-aware split as `__fn(...)`. Substitute type-params into each
    // field annotation (string-level word-boundary substitution) and
    // recursively parse to get field types, then intern the layout into
    // module.struct_layouts.
    if let Some(open_idx) = s.find('<')
        && s.ends_with('>')
        && let Some(t) = generic::parse_generic(
            s,
            open_idx,
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        )
    {
        return t;
    }
    parse_keyword(s, aliases)
}

/// `T[]` array-suffix arm of [`parse_type`] — recurse on the element
/// type, intern, return Arr. The flat string is produced by
/// parser::parse_type_ann, so stripping a trailing "[]" recurses
/// cleanly; multi-dim arrays (`T[][]`) work via the recursion:
/// `number[][]` → strip to `number[]` → strip to `number` → I64,
/// interned outer-to-inner.
///
/// Chunk 733 — a fn-typed array element is Closure-repr, mirror of
/// the struct-field `__cls` tagging: the slot is mutable and can hold
/// capturing closures (`fns.push(() => s)`), which are env pointers —
/// dispatching one as a raw FnSig fn address jumps into the env block
/// (SIGBUS). Named-fn store-sites are wrapped by the fn-arr axes in
/// `ast_collect_fn_closure` so both shapes reach the slot as closure
/// cells.
fn parse_arr_suffix(
    elem_ann: &str,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Type {
    let elem = parse_type(
        Some(elem_ann),
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
    );
    let elem = match elem {
        Type::FnSig(sig) => Type::Closure(sig),
        e => e,
    };
    let id = intern_arr_layout(arr_layouts, elem);
    Type::Arr(id)
}

/// Flat scalar / keyword annotation tail (`number` / `string` / `Map`
/// / ... plus the alias fallback) — split from `parse_type`
/// (2026-07-03: the chunk-437 split left the fn at 201 LOC, one over
/// the 200 hard limit; match body verbatim).
fn parse_keyword(s: &str, aliases: &HashMap<String, Type>) -> Type {
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
        // `Date` is the TS-spelled annotation; `date` is the internal
        // spelling type_to_ann emits. Mirrors the check_type_ann arm.
        "date" | "Date" => Type::Date,
        // T-21 — `fetch(url)` Response heap struct; ptr at SSA.
        "Response" => Type::Ptr,
        // T-10.a — Any plumbing; single 64B ptr slot, tag via heap header.
        "any" => Type::Any,
        // TS `object` collapses to Type::Any (non-primitive constraint
        // is independent substrate; mirror of `check_type_ann.rs`).
        "object" => Type::Any,
        // TS `unknown` — top type, alias of Type::Any at the runtime
        // layer (mirror of `check_type_ann.rs`). No-access-without-
        // narrow constraint is independent L3b.
        "unknown" => Type::Any,
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
