//! Type-annotation string → `ssa::Type` resolution (`parse_type`),
//! split out of ssa_lower.rs. Generic struct instantiations resolve
//! through a reserve-first persistent memo (`inst_memo`) so recursive
//! aliases like `type Rec<T> = { next: Rec<T> | null }` close their
//! back-edge on the reserved nominal sid — see
//! rfcs/20260612-generic-recursive-alias.

use crate::ast::{GenericStructDecls, PropKey};
use std::collections::HashMap;

mod absent;
mod generic;
mod keyword;
mod markers;
pub(crate) use absent::{nullable_inner_boxes, slot_is_any};
use keyword::parse_keyword;

use crate::ssa::{self, Type};
use crate::ssa_lower::intern_arr_layout;

/// The annotation of an optional `number` member, as the parser spells
/// it.
pub(crate) const OPTIONAL_NUMBER_ANN: &str = "__nullable(number)";

/// The slot of `T | null` once `T` has resolved: an alias resolving to
/// a scalar has nowhere to put the null either, so it boxes like the
/// spelled-out `number` / `boolean` ([`nullable_inner_boxes`]).
fn nullable_slot(inner: Type) -> Type {
    match inner {
        Type::I64 | Type::I32 | Type::F64 | Type::Bool => Type::Any,
        t => t,
    }
}

/// RFC 20260710-optional-undefined-repr C4 — struct FIELD-position
/// type resolution. A `__nullable(number|boolean)` field slot
/// materializes as `Type::Any` (8B NaN-box): scalar slots have no
/// in-band undefined/null encoding, so the optional slot pays the
/// box tax (undefined = ANY_UNDEF, null = ANY_NULL, values box as
/// i64/f64/bool) while non-optional slots stay raw. Param / let /
/// return positions keep the plain [`parse_type`] strip — their ABI
/// is unchanged. A `__nullable(<alias>)` whose alias RESOLVES to a
/// scalar boxes the same way, through [`parse_type`]; every other
/// spelling delegates verbatim.
///
/// The exception is a plain optional `number` member (`a?: number`,
/// `plain_optional`: the written type spells neither `null` nor
/// `undefined`): it keeps an f64 slot, and `undefined` is the slot's
/// sentinel (`F64_UNDEF_SENTINEL_BITS`), which every consumer of a
/// `number` field that may hold one already reads. The cell's absent
/// set says whether the member is an own property. `a?: number | null`
/// may hold `null`, which an f64 has no spelling for, so it boxes.
pub(crate) fn parse_struct_field_type(
    ann: &str,
    plain_optional: bool,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &GenericStructDecls,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Type {
    if plain_optional && ann == OPTIONAL_NUMBER_ANN {
        return Type::F64;
    }
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
    generic_struct_decls: &GenericStructDecls,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Type {
    let s = match ann {
        Some(s) => s,
        None => return Type::Void,
    };
    // the lib.d.ts names of what exec / match answer
    let s = match s {
        "RegExpExecArray" | "RegExpMatchArray" => "string[]",
        s => s,
    };
    // A scalar `T | null` has nowhere to PUT the null: the in-band 0
    // sentinel is a legitimate `0` / `false`, so `a === null`, `typeof`
    // and narrowing all answer the value's answer. It boxes as Any
    // (RFC 20260710-optional-undefined-repr C4, the struct-field rule).
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
        return nullable_slot(parse_type(
            Some(inner),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        ));
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
    if matches!(s, "ArrayBuffer" | "DataView" | "FinalizationRegistry")
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
    // - P3.closure-in-struct-field `__cls(P)->(R)`, tagged by the
    //   `tag_struct_field_closure_types` desugar pass: struct fields
    //   can store both FnSig (forwarder-wrapped at construction) and
    //   capturing Closure values, so the slot is Closure-typed.
    //   Fn-typed param / return / let bindings keep `__fn(P)->(R)` →
    //   Type::FnSig via try_parse_fn_type below, preserving direct
    //   dispatch on the hot fn-as-callback path.
    // - RFC 20260714-objlit-accessor `__mth(P)->(R)`: an object-literal
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
    // M2 Phase B Stage 2 — fn type `__fn(P1|P2|...)->(R)` decoder lives
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
///
/// An element spelling `T | null` rides the any lane for EVERY T, not
/// just the two [`nullable_inner_boxes`] scalars. For a pointer-shaped
/// T the slot itself is fine — its null is the in-band 0 a pointer
/// always had spare — but the typed element lane has no way to SAY so:
/// `const a: (O | null)[] = [null, {x: 1}]` died at runtime with
/// "array element does not match the annotated element type", and the
/// empty-then-push spelling printed `[unknown-any-tag]`. That is the
/// same half-rule the inferred array literal had until the lane gate
/// there was widened to every `Nullable(_)`; this is its annotated
/// half. The two sides now agree, which is also what lets the
/// annotated decl skip `__torajs_arr_any_to_typed` entirely (its
/// target layout is Any, so there is nothing to convert to).
fn parse_arr_suffix(
    elem_ann: &str,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &GenericStructDecls,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
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
    let elem = if elem_ann.trim_start().starts_with("__nullable(") {
        Type::Any
    } else {
        elem
    };
    let id = intern_arr_layout(arr_layouts, elem);
    Type::Arr(id)
}
