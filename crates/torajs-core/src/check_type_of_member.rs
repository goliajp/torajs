//! `Expr::Member` typecheck extracted from
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Member` arm
//! (chunk 167).
//!
//! Pre-extract this arm was 1939 LOC inside `type_of_inner`. Body
//! verbatim moves here as a `check` free fn taking `&mut Checker`;
//! `type_of_inner`'s arm delegates with one line. This is the
//! largest single arm in type_of_inner; chunk 167's sibling itself
//! is over the 500-LOC file-size HARD limit (~1930 LOC) and is
//! registered in `torajs-file-size-debt.md` as known-debt. Per-
//! property-family sub-siblings are deferred to a follow-up
//! rotation — this chunk just gets the LOC out of check.rs so the
//! rest of the god-fn decomposition can continue.
//!
//! Body unchanged — handles every member access shape:
//! - M-OO.5 visibility enforcement (Public/Private/Protected for
//!   `this.x` and `let x: ClassName = ...; x.field`)
//! - Class instance method / field / accessor (getter) lookups
//! - String method dispatch (`s.length` / `s.charCodeAt` / `s.split` / ...)
//! - Array method dispatch (`xs.length` / `xs.push` / `xs.map` / ...)
//! - Number / Boolean / BigInt / Date / RegExp / Promise / Map / Set
//!   / WeakRef / WeakMap / WeakSet / MapIter / ArrIter / Symbol
//!   per-type method tables
//! - Static class members (`Math.floor` / `ClassName.STATIC_FIELD`)
//! - Any / Function fallbacks at the end

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type, resolve_class_ref};
use crate::check_type_of_member_accessor::{class_name_of, enforce_visibility};

/// `lenient_missing` — S2.24 刀 4: true for a desugar-minted
/// default-guarded pattern load (`Ast::dstr_default_member_loads`);
/// the terminal "no member" reject answers `Type::Any` instead — the
/// read becomes a RUNTIME GetV (§13.15.5.4) at the lowering, because
/// a static miss is not a runtime miss: a prefix-compatible
/// heterogeneous array types its elements by the anchor
/// (`[{}, {b: 3}]` → Struct([])), and the wider element really
/// carries the field. Every other error path (visibility, family-arm
/// rejects) stays an error.
pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    obj: &ExprId,
    name: &str,
    lenient_missing: bool,
) -> Result<Type, String> {
    // RFC 20260710 C5 — a member-path truthiness narrow
    // (`if (o.cb) { o.cb() }`) overrides the declared Nullable
    // field type inside the guarded branch. Keys are canonical
    // receiver paths (chunk 789: Ident / Member chain), minted from
    // the same shape.
    if let Some(path) = crate::check_assigns_to::member_path(ast, *obj)
        && let Some(narrowed) = checker.member_narrows.get(&(path, name.to_string()))
    {
        return Ok(narrowed.clone());
    }
    let obj_ty = checker.type_of(ast, *obj)?;
    // RFC 20260806 — a builtin method this module might have patched or
    // deleted is not the kernel's to answer. Typing the read as Any
    // routes the call through `ssa_lower_any_method_call`'s cluster-#4
    // branch (concrete receiver, Any member read), i.e. the runtime
    // dispatcher that consults the patch bitmap and the tombstones —
    // which is the lane an `any`-typed receiver has always taken and the
    // only reason a typed receiver could never see a patch. The bitmap
    // is read when the call runs, so a call sequenced before the patch
    // still answers from the kernel.
    if !checker.proto_shadow.is_empty()
        && let Some(family) = crate::builtin_proto_shadow::family_of(&obj_ty)
        && checker.proto_shadow.shadows(family, name)
    {
        return Ok(Type::Any);
    }
    // RC-4 F1a — Nullable<Array<T>> receiver (un-narrowed
    // exec/match result) decays to the bare array for member
    // lookup; the null case is a runtime TypeError at the
    // lowering-side guard, matching JS null-deref semantics.
    // Other Nullable receivers keep the existing reject.
    let obj_ty = match obj_ty {
        Type::Nullable(inner) if matches!(*inner, Type::Array(_)) => *inner,
        other => other,
    };
    enforce_visibility(checker, ast, obj, name)?;
    // Struct field access is the most general path — look up
    // the named field; type is whatever it was declared as.
    // V3-05 — resolve any ClassRef placeholder embedded in
    // obj_ty (self-reference fields hit this).
    let resolved_obj_ty = resolve_class_ref(
        &obj_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    if let Type::Struct(fields) = &resolved_obj_ty
        && let Some((_, ty)) = fields.iter().find(|(fname, _)| fname == name)
    {
        // RFC 20260715-nominal-class-identity — hand back the field's
        // type AS DECLARED. Unwrapping a `ClassRef` here would strip the
        // class name off every field holding an instance, and the next
        // member access off it would have no class to look a method up
        // in (`yield*` lifts its delegate iterator into exactly such a
        // field, then calls `.next()` on it). Structural consumers
        // resolve on their own.
        return Ok(ty.clone());
    }
    // Accessor reads (objlit `__getter_` slot + class
    // `accessor_getters` probe) — see the sibling's doc.
    if let Some(r) = crate::check_type_of_member_accessor::try_accessor_read(
        checker,
        ast,
        &obj_ty,
        &resolved_obj_ty,
        name,
    ) {
        return r;
    }
    /* T-15.g.2 (v0.5.0) — built-in `Promise<T>.value` returns
     * T. The parser desugars `await p` to `p.value` (Phase L
     * MVP — synchronous read of the resolved value), so this
     * Member-access rule is the entire `await` typing for
     * built-in promises. ssa_lower's matching arm emits
     * `__torajs_promise_get_value(p)` which reads the i64
     * value slot from the Promise heap block. The user-class
     * Promise pattern keeps working since Type::Object
     * structs go through the field-lookup branch above. */
    if let Type::Promise(inner) = &obj_ty
        && name == "value"
    {
        return Ok((**inner).clone());
    }
    /* P10.4 — `await e` on non-Promise e per ES spec:
     * conceptually `Promise.resolve(e)` is constructed and
     * its resolved value is yielded — which for any
     * non-thenable e collapses to e itchecker. The parser
     * desugars `await e` to `e.value`; this arm treats
     * `.value` as identity for the types that can never
     * carry a real `value` field of their own (primitives
     * + the built-in heap container types). The Promise
     * arm above takes precedence for actual Promise<T>;
     * the user-struct field-lookup arm below takes
     * precedence for declared `{ value: T }` Struct
     * shapes. Type::Object("Symbol" / etc.), Type::Class
     * and Type::Struct intentionally fall through —
     * those CAN have a real `.value` member. */
    if name == "value"
        && matches!(
            obj_ty,
            Type::Number | Type::String | Type::Boolean | Type::Array(_) | Type::BigInt
        )
    {
        return Ok(obj_ty);
    }
    // Phase I.1 — class method. Probe `__cm_<class>__<name>` in
    // globals; if found, return its Function type with the implicit
    // `__this` first param stripped (the caller's args fill the rest).
    // Used by sibling-method calls left un-rewritten by desugar.
    //
    // RFC 20260715-nominal-class-identity — the class comes from the
    // receiver's NAME. This used to reverse-look-up "the first aliased
    // class with my struct shape", so `{a: 1}` could call
    // `class C { a; m() }`'s method and get its result.
    if let Some(cname) = class_name_of(&obj_ty, ast) {
        let cm_name = format!("__cm_{cname}__{name}");
        if let Some(Type::Function(params, ret)) = checker.globals.get(&cm_name) {
            // Strip the implicit `__this` first param — plus the
            // knife-4a `__torajs_real_argc` / `__torajs_argv`
            // synthetics when the body took the runtime argv face
            // (the adapter feeds those; a caller never spells them).
            let skip = if ast.method_argv_fns.contains(&cm_name) {
                3
            } else {
                1
            };
            if params.len() >= skip {
                let user_params = params[skip..].to_vec();
                return Ok(Type::Function(user_params, ret.clone()));
            }
        }
    }
    // Structural families (Array / Map / Date / ...) read the resolved
    // shape — a class instance reaches its struct's members too.
    if let Some(r) = try_family_dispatch(&resolved_obj_ty, name) {
        return r;
    }
    // Iterator-helper fallback face — a §27.1.4 helper name on an
    // iterator-by-construction receiver (generator class / extends-
    // Iterator heir / MapIter / ArrIter) answers Any and rides the
    // any-lane method dispatcher. Keys on the UNRESOLVED obj_ty:
    // resolution flattens ClassRef to its Struct and loses the name.
    if let Some(r) = crate::check_type_of_member_iterator::try_match(&obj_ty, name, ast) {
        return r;
    }
    // Every other `(obj_ty, name)` shape has already been
    // matched by one of the per-type-family `try_match`
    // siblings inside [`try_family_dispatch`] (chunks
    // 191-206). Anything reaching this point is genuinely
    // unknown for the obj_ty — emit a typecheck error.
    if lenient_missing {
        return Ok(Type::Any);
    }
    // RFC 20260804-method-rebind-generic-body blade 4 — a CLASS
    // INSTANCE receiver's terminal miss is not an error: §10.1.8.1
    // [[Get]] on an absent property answers undefined (bun runs
    // these programs). Answer Any; the lowering's struct-field miss
    // arm boxes the receiver and rides the any-member lane (runtime
    // GetV — an expando / prototype write may have landed the name).
    // Anonymous Struct shapes keep the loud reject: an object
    // literal's fields are all statically known, so a miss there is
    // overwhelmingly a typo (recorded diagnostic-posture boundary).
    // A `__priv_<cls>__<name>` miss stays a compile-time reject: an
    // undeclared private name is an early SyntaxError (§13.1 all
    // private references must resolve lexically), never a runtime
    // undefined — the mangled prefix is minted only by the parser's
    // PrivateIdent path, so the gate is mechanical.
    if matches!(obj_ty, Type::ClassRef(_)) {
        if let Some(rest) = name.strip_prefix("__priv_") {
            let field = rest.split_once("__").map(|(_, f)| f).unwrap_or(rest);
            return Err(format!(
                "private field `#{field}` is not declared in this class"
            ));
        }
        return Ok(Type::Any);
    }
    // Rotation 346 — a ZERO-field anonymous Struct is the
    // `var o = {}` shape: it declares no surface a typo could miss,
    // and it only grows through runtime writes (expando stores, an
    // apply/call thisArg receiver) — §10.1.8.1 [[Get]] on an absent
    // property answers undefined, exactly the ClassRef posture
    // above. Answer Any; the lowering boxes the receiver and rides
    // the any-member lane. A NON-empty literal keeps the loud
    // reject: its fields ARE statically known, so a miss there is
    // overwhelmingly a typo (the recorded diagnostic-posture
    // boundary).
    if matches!(obj_ty, Type::Struct(ref f) if f.is_empty()) {
        return Ok(Type::Any);
    }
    Err(format!("no member `.{name}` on type {obj_ty:?}"))
}

/// Per-type-family `try_match` dispatch chain (chunks 191-206).
/// Every sibling reads only `(obj_ty, name)` — no checker state.
/// The full sibling dispatch table:
///   191 Date / 192 Weak{Ref,Map,Set} / 193 Set ∪ Map ∪
///   MapIter ∪ ArrIter / 194 Number ∪ Boolean ∪ BigInt ∪
///   Symbol prim methods / 195 RegExp / 196 Array (incl.
///   chunk-206 Array-as-Object catch-all) / 197 String /
///   198 Promise / 199 Struct ∪ Function ∪ Any misc /
///   200 Object("console" | "Math" | "Number" | "BigInt"
///   | "JSON" | "Array" | "Promise" | "Symbol") +
///   single-tag Object static / 201 Object("Date" | "Bun"
///   | "BunFile" | "Response" | "process" | "env" |
///   "process_stdout" | "process_stderr" | "fs" |
///   "fs_promises" | "String") I/O namespaces / 202
///   Object("Object") accessor / mutating / property-
///   descriptor / 203 Object("Object") ∪ Object("Reflect")
///   mixed (keys / hasOwn / Reflect.get) / 204 generic
///   Object(_) catch-alls (hasOwnProperty / prototype /
///   name / length / etc.) + Symbol singletons / 205
///   prim ∪ Any unions (String|Array length, prim
///   constructor, prim|Any hasOwnProperty).
fn try_family_dispatch(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    // Date instance methods — see
    // [`crate::check_type_of_member_date`] (chunk 191 —
    // first sub-batch of check_type_of_member per-type-
    // family decomposition).
    if matches!(obj_ty, Type::Date)
        && let Some(r) = crate::check_type_of_member_date::try_match(name)
    {
        return Some(r);
    }
    // Weak family (WeakRef / WeakMap / WeakSet) instance
    // methods — see [`crate::check_type_of_member_weak`]
    // (chunk 192 — second sub-batch).
    if matches!(obj_ty, Type::WeakRef | Type::WeakMap | Type::WeakSet)
        && let Some(r) = crate::check_type_of_member_weak::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // Set / Map / MapIter / ArrIter instance methods — see
    // [`crate::check_type_of_member_setmap`] (chunk 193 —
    // third sub-batch).
    if matches!(
        &obj_ty,
        Type::Map | Type::Set | Type::MapIter | Type::ArrIter
    ) && let Some(r) = crate::check_type_of_member_setmap::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // Primitive wrappers (Number / Boolean / BigInt / Symbol)
    // single-type arms — see
    // [`crate::check_type_of_member_prim`] (chunk 194 —
    // fourth sub-batch). Mixed arms (`(Number | String |
    // Boolean | BigInt | Symbol, "constructor")` etc.) stay
    // in the main match so the String / Any branches are
    // still served when this dispatch returns None.
    if matches!(
        &obj_ty,
        Type::Number | Type::Boolean | Type::BigInt | Type::Symbol
    ) && let Some(r) = crate::check_type_of_member_prim::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // RegExp instance methods + properties — see
    // [`crate::check_type_of_member_regex`] (chunk 195 —
    // fifth sub-batch).
    if matches!(obj_ty, Type::RegExp)
        && let Some(r) = crate::check_type_of_member_regex::try_match(name)
    {
        return Some(r);
    }
    // `Type::Array` instance methods + the Array-as-Object
    // catch-all (`arr.x` for unknown `x` → Type::Any) — see
    // [`crate::check_type_of_member_array`] (chunk 196 —
    // sixth sub-batch; chunk 206 folded the catch-all in as
    // the last sibling arm). The shared `(Type::String,
    // "length") | (Type::Array(_), "length")` arm stays in
    // the main match (handled by chunk 205 prim-union).
    if matches!(obj_ty, Type::Array(_))
        && let Some(r) = crate::check_type_of_member_array::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::String` instance methods — see
    // [`crate::check_type_of_member_string`] (chunk 197 —
    // seventh sub-batch). Mixed-type arms (the shared
    // `length` arm, prim-union `constructor`, and
    // prim+Any union `hasOwnProperty` /
    // `propertyIsEnumerable`) stay in the main match.
    if matches!(obj_ty, Type::String)
        && let Some(r) = crate::check_type_of_member_string::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::Promise` instance methods — see
    // [`crate::check_type_of_member_promise`] (chunk 198 —
    // eighth sub-batch). The pre-match structural getter
    // forwarding (`if let Type::Promise(inner) = obj_ty`)
    // stays in the main module; only the 3 method arms
    // (`then` / `catch` / `finally`) move.
    if matches!(obj_ty, Type::Promise(_))
        && let Some(r) = crate::check_type_of_member_promise::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::Struct` / `Type::Function` / `Type::Any`
    // single-type members + per-type catch-alls — see
    // [`crate::check_type_of_member_misc`] (chunk 199 —
    // ninth sub-batch). The mixed-type prim+Any
    // `hasOwnProperty` / `propertyIsEnumerable` arm and
    // the `Array(_), name` catch-all stay in the main
    // match (their patterns aren't single-type).
    if matches!(obj_ty, Type::Struct(_) | Type::Function(..) | Type::Any)
        && let Some(r) = crate::check_type_of_member_misc::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::Object("NAMESPACE")` static-namespace members
    // (console / Math / Number / BigInt / JSON / Array /
    // Object single-type / Promise / Symbol) — see
    // [`crate::check_type_of_member_namespace`] (chunk 200 —
    // tenth sub-batch). Mixed-namespace arms (Object ∪
    // Reflect for keys / hasOwn / ownKeys, generic
    // Object(_) catch-alls, Object mutating /
    // accessor / property-descriptor methods) stay in the
    // main match because their patterns span more than one
    // namespace tag or carry richer dispatch.
    if let Type::Object(_) = obj_ty
        && let Some(r) = crate::check_type_of_member_namespace::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // I/O-flavored namespaces — see
    // [`crate::check_type_of_member_namespace_io`] (chunk
    // 201 — eleventh sub-batch). Sibling to chunk 200;
    // covers Date / Bun / BunFile / Response / process /
    // env / process_stdout-stderr / fs / fs_promises /
    // String static namespace tags.
    if let Type::Object(_) = obj_ty
        && let Some(r) = crate::check_type_of_member_namespace_io::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // `Type::Object("Object")` accessor / mutating /
    // property-descriptor arms (getPrototypeOf /
    // defineProperty / getOwnPropertyDescriptor /
    // setPrototypeOf / defineProperties / create / assign /
    // preventExtensions / seal / isExtensible / isSealed)
    // — see [`crate::check_type_of_member_object_meta`]
    // (chunk 202 — twelfth sub-batch). Mixed Object/Reflect
    // arms + generic `(Type::Object(_), …)` catch-alls stay
    // in the main match.
    if matches!(obj_ty, Type::Object("Object"))
        && let Some(r) = crate::check_type_of_member_object_meta::try_match(name)
    {
        return Some(r);
    }
    // Mixed `Type::Object("Object")` ∪ `Type::Object("Reflect")`
    // arms (`keys` ∪ `getOwnPropertyNames` ∪ `ownKeys`,
    // `hasOwn` ∪ `has`, `Reflect.get`) — see
    // [`crate::check_type_of_member_reflect`] (chunk 203 —
    // thirteenth sub-batch). Generic `(Type::Object(_), …)`
    // catch-alls (hasOwnProperty / propertyIsEnumerable /
    // isPrototypeOf / toString / prototype / name / length)
    // stay in the main match — they fire for every namespace
    // tag, not just Object/Reflect.
    if matches!(obj_ty, Type::Object("Object") | Type::Object("Reflect"))
        && let Some(r) = crate::check_type_of_member_reflect::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // Generic `(Type::Object(_), …)` Object.prototype catch-alls
    // (V3-18 m2.b — hasOwnProperty / propertyIsEnumerable /
    // isPrototypeOf / toString — subset stubs that fire for
    // every namespace tag) + constructor introspection (V3-18
    // m2.c — prototype / name / length) + Symbol singleton
    // accessors (T-13.c — `Symbol.iterator` / `asyncIterator` /
    // `toPrimitive`, which gate on `Type::Object("Symbol")`).
    // See [`crate::check_type_of_member_object_generic`]
    // (chunk 204 — fourteenth sub-batch). All other
    // `Type::Object(...)` arms have already been handled by
    // the more-specific pre-match dispatches above (chunks
    // 200 namespace / 201 namespace_io / 202 object_meta /
    // 203 reflect); the generic catch-alls fire last for any
    // namespace tag not picked up earlier.
    if let Type::Object(_) = obj_ty
        && let Some(r) = crate::check_type_of_member_object_generic::try_match(obj_ty, name)
    {
        return Some(r);
    }
    // Mixed-primitive ∪ Any union arms (the cross-type-family
    // arms whose `|`-union spans more than one primitive
    // — `(String, "length") | (Array, "length")`, prim-union
    // `constructor`, prim+Any union `hasOwnProperty` /
    // `propertyIsEnumerable`). See
    // [`crate::check_type_of_member_prim_union`] (chunk 205 —
    // fifteenth sub-batch). Cross-family `|`-union patterns
    // can't live in any single primitive's dedicated sibling.
    crate::check_type_of_member_prim_union::try_match(obj_ty, name)
}
