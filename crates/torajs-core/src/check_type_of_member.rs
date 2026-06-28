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

use crate::ast::Visibility;
use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type, is_array_method_name, resolve_class_ref};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    obj: &ExprId,
    name: &str,
) -> Result<Type, String> {
    let obj_ty = checker.type_of(ast, *obj)?;
    // M-OO.5 — visibility enforcement. Find the binding's
    // nominal class:
    //   - `this` inside a class method body inherits the
    //     current class context.
    //   - An Ident bound by `let x: ClassName = ...` carries
    //     its `declared_class` from the LetDecl arm.
    // Other shapes (chained Member, Call result, etc.)
    // currently get no nominal info; treat their visibility
    // as Public until that path needs tightening.
    let obj_class: Option<String> = match ast.get_expr(*obj) {
        Expr::This => checker.current_class.clone(),
        Expr::Ident(n) => checker.lookup(n).and_then(|info| info.declared_class),
        _ => None,
    };
    if let Some(cls) = obj_class.as_deref()
        && let Some(vis) = ast
            .member_visibility
            .get(&(cls.to_string(), name.to_string()))
            .copied()
    {
        let allowed = match vis {
            Visibility::Public => true,
            Visibility::Private => checker.current_class.as_deref() == Some(cls),
            Visibility::Protected => checker
                .current_class
                .as_deref()
                .map(|c| c == cls || checker.is_descendant_of(ast, c, cls))
                .unwrap_or(false),
        };
        if !allowed {
            return Err(format!(
                "M-OO.5: cannot access {vis:?} member `{cls}.{name}` from {}",
                checker
                    .current_class
                    .as_deref()
                    .map(|c| format!("class `{c}`"))
                    .unwrap_or_else(|| "outside any class".to_string())
            ));
        }
    }
    // Struct field access is the most general path — look up
    // the named field; type is whatever it was declared as.
    // V3-05 — resolve any ClassRef placeholder embedded in
    // obj_ty (self-reference fields hit this).
    let resolved_obj_ty =
        resolve_class_ref(&obj_ty, &checker.aliases, &checker.generic_alias_decls);
    if let Type::Struct(fields) = &resolved_obj_ty
        && let Some((_, ty)) = fields.iter().find(|(fname, _)| fname == name)
    {
        return Ok(resolve_class_ref(
            ty,
            &checker.aliases,
            &checker.generic_alias_decls,
        ));
    }
    // P8.2 — accessor read: `c.value` where the resolved
    // class C has a `get value(): T` declaration. After the
    // struct-field lookup misses (accessors aren't fields),
    // probe `accessor_getters` for the receiver's class and
    // return the getter's declared return type. ssa_lower
    // emits a `Call(__cm_<C>__value_get, c)` at the
    // matching Member arm — type-wise we just return the
    // getter's `ret` so caller sites see a normal value
    // (not a Function), matching ES §10.1.7 [[Get]]
    // semantics.
    if let Type::Struct(_) = &resolved_obj_ty {
        let mut accessor_class: Option<String> = None;
        for (n, ty) in checker.aliases.iter() {
            if *ty == resolved_obj_ty && ast.class_parents.contains_key(n) {
                accessor_class = Some(n.clone());
                break;
            }
        }
        if let Some(cls) = accessor_class
            && let Some(getter_fn) = ast.accessor_getters.get(&(cls.clone(), name.to_string()))
            && let Some(Type::Function(_params, ret)) = checker.globals.get(getter_fn)
        {
            return Ok(resolve_class_ref(
                ret,
                &checker.aliases,
                &checker.generic_alias_decls,
            ));
        }
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
    // Phase I.1 — class method on Type::Struct. Reverse-lookup
    // the class name from the struct shape (matches the
    // first-aliased class with that struct), then probe
    // `__cm_<class>__<name>` in globals. If found, return
    // its Function type with `__this` (the implicit first
    // param) stripped — caller's args fill the remaining
    // params. Used by sibling-method calls left
    // un-rewritten by desugar (the chain-and-static cases
    // were rewritten into Ident calls already).
    if let Type::Struct(_) = &obj_ty {
        let mut class_name: Option<String> = None;
        for (n, ty) in checker.aliases.iter() {
            if *ty == obj_ty && ast.class_parents.contains_key(n) {
                class_name = Some(n.clone());
                break;
            }
        }
        if let Some(cname) = class_name {
            let cm_name = format!("__cm_{cname}__{name}");
            if let Some(Type::Function(params, ret)) = checker.globals.get(&cm_name) {
                // Strip the implicit `__this` first param.
                if !params.is_empty() {
                    let user_params = params[1..].to_vec();
                    return Ok(Type::Function(user_params, ret.clone()));
                }
            }
        }
    }
    // Date instance methods — see
    // [`crate::check_type_of_member_date`] (chunk 191 —
    // first sub-batch of check_type_of_member per-type-
    // family decomposition).
    if matches!(&obj_ty, Type::Date)
        && let Some(r) = crate::check_type_of_member_date::try_match(name)
    {
        return r;
    }
    // Weak family (WeakRef / WeakMap / WeakSet) instance
    // methods — see [`crate::check_type_of_member_weak`]
    // (chunk 192 — second sub-batch).
    if matches!(&obj_ty, Type::WeakRef | Type::WeakMap | Type::WeakSet)
        && let Some(r) = crate::check_type_of_member_weak::try_match(&obj_ty, name)
    {
        return r;
    }
    // Set / Map / MapIter / ArrIter instance methods — see
    // [`crate::check_type_of_member_setmap`] (chunk 193 —
    // third sub-batch).
    if matches!(
        &obj_ty,
        Type::Map | Type::Set | Type::MapIter | Type::ArrIter
    ) && let Some(r) = crate::check_type_of_member_setmap::try_match(&obj_ty, name)
    {
        return r;
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
    ) && let Some(r) = crate::check_type_of_member_prim::try_match(&obj_ty, name)
    {
        return r;
    }
    // RegExp instance methods + properties — see
    // [`crate::check_type_of_member_regex`] (chunk 195 —
    // fifth sub-batch).
    if matches!(&obj_ty, Type::RegExp)
        && let Some(r) = crate::check_type_of_member_regex::try_match(name)
    {
        return r;
    }
    // `Type::Array` instance methods — see
    // [`crate::check_type_of_member_array`] (chunk 196 —
    // sixth sub-batch). The shared
    // `(Type::String, "length") | (Type::Array(_), "length")`
    // arm and the catch-all
    // `(Type::Array(_), name) if name != "length"
    //   && !is_array_method_name(name)` arm stay in the main
    // match because their patterns aren't Array-only.
    if matches!(&obj_ty, Type::Array(_))
        && let Some(r) = crate::check_type_of_member_array::try_match(&obj_ty, name)
    {
        return r;
    }
    match (&obj_ty, name) {
    (Type::Object("console"), m)
        if matches!(m, "log" | "error" | "warn" | "info" | "debug") =>
    {
        // S328 — WHATWG console §1.1.{2,4}: `info` /
        // `debug` print to the same stream as `log`.
        // bun aliases info/debug to log (stdout); tr
        // routes through the same `print_*` intrinsic
        // family in ssa_lower.
        Ok(Type::Function(vec![Type::Any], Box::new(Type::Void)))
    }
    // `Math` global — every method takes one number and
    // returns a number. f64-flavored at the SSA level
    // (the lowerer auto-promotes integer args), but
    // check.rs uses the umbrella Type::Number.
    (Type::Object("Math"), m)
        if matches!(
            m,
            "sqrt" | "abs" | "floor" | "ceil" | "log" | "exp"
            | "sign" | "round" | "trunc"
            | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
            | "log2" | "log10" | "cbrt"
            | "sinh" | "cosh" | "tanh" | "asinh" | "acosh" | "atanh"
            | "expm1" | "log1p" | "clz32" | "fround" | "f16round"
        ) =>
    {
        Ok(Type::Function(vec![Type::Number], Box::new(Type::Number)))
    }
    (Type::Object("Math"), "imul") => Ok(Type::Function(
        vec![Type::Number, Type::Number],
        Box::new(Type::Number),
    )),
    // ES2025 §21.3.2.32 — correctly-rounded sum of an
    // Array<Number>. Narrow form: only Array<Number>
    // input (spec accepts any iterable of Number;
    // tora's Set/Map/iterator surface comes later).
    (Type::Object("Math"), "sumPrecise") => Ok(Type::Function(
        vec![Type::Array(Box::new(Type::Number))],
        Box::new(Type::Number),
    )),
    (Type::Object("Math"), "random") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::Number),
    )),
    // Two-arg methods: pow(x, y), min(a, b), max(a, b),
    // atan2(y, x).
    (Type::Object("Math"), m)
        if matches!(m, "pow" | "min" | "max" | "atan2") =>
    {
        Ok(Type::Function(
            vec![Type::Number, Type::Number],
            Box::new(Type::Number),
        ))
    }
    // Constants — read directly without parens.
    (Type::Object("Math"), m)
        if matches!(
            m,
            "PI" | "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E"
            | "SQRT2" | "SQRT1_2"
        ) =>
    {
        Ok(Type::Number)
    }
    // Number namespace constants — common floating-point
    // limits and integer-safety bounds.
    (Type::Object("Number"), m)
        if matches!(
            m,
            "NaN" | "POSITIVE_INFINITY" | "NEGATIVE_INFINITY"
            | "EPSILON" | "MAX_SAFE_INTEGER" | "MIN_SAFE_INTEGER"
            | "MAX_VALUE" | "MIN_VALUE"
        ) =>
    {
        Ok(Type::Number)
    }
    // `Number` global — parseInt / parseFloat coerce a
    // string to a number; isInteger / isNaN / isFinite
    // are unary number predicates.
    (Type::Object("Number"), "parseInt") => Ok(Type::Function(
        vec![Type::String, Type::Number],
        Box::new(Type::Number),
    )),
    (Type::Object("Number"), "parseFloat") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Number),
    )),
    (Type::Object("Number"), m)
        if matches!(m, "isInteger" | "isNaN" | "isFinite" | "isSafeInteger") =>
    {
        Ok(Type::Function(vec![Type::Number], Box::new(Type::Boolean)))
    }
    // P12.4-B/C — `BigInt.asIntN(bits, value)` /
    // `BigInt.asUintN(bits, value)` per ES §21.2.2.1 /
    // §21.2.2.2. `bits` is a `Number` (Index per spec;
    // tora's `Number` covers integer-shaped values
    // already); `value` is `BigInt`; returns BigInt.
    (Type::Object("BigInt"), "asIntN") | (Type::Object("BigInt"), "asUintN") => {
        Ok(Type::Function(
            vec![Type::Number, Type::BigInt],
            Box::new(Type::BigInt),
        ))
    }
    // V3-18 m2.b — Object.prototype methods on
    // constructor-namespace objects (Number / String /
    // Boolean / Array / etc). Same subset semantics as
    // m2.a on primitives: hasOwnProperty /
    // propertyIsEnumerable always false (no own enum
    // properties tracked), valueOf identity.
    (Type::Object(_), "hasOwnProperty")
    | (Type::Object(_), "propertyIsEnumerable") => {
        Ok(Type::Function(vec![Type::String], Box::new(Type::Boolean)))
    }
    (Type::Object(_), "isPrototypeOf") => {
        Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean)))
    }
    (Type::Object(_), "toString") => {
        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
    }
    // V3-18 m2.c → 2026-05-18 — `Number.prototype` /
    // `String.prototype` / etc — every constructor
    // object has a `.prototype` property. Subset
    // returns Type::Any so subsequent `.X` access
    // routes through dynobj_get (returning ANY_UNDEF
    // for unknown fields, harmless when consumed by
    // a verifyProperty-style stub). Pre-fix Type::Null
    // blocked `verifyProperty(X.prototype.Y, ...)` —
    // the dominant test262 shape — at typecheck time.
    // typeof X.prototype still works via the typeof-
    // namespace-member arm above.
    (Type::Object(_), "prototype") => Ok(Type::Any),
    (Type::Object(_), "name") => Ok(Type::String),
    (Type::Object(_), "length") => Ok(Type::Number),
    // JSON.stringify(value) — value can be any subset
    // type; result is String. The actual type-aware
    // serialization shape happens at lower-time
    // (per-call-site monomorphization).
    (Type::Object("JSON"), "stringify") => {
        Ok(Type::Function(vec![Type::Any], Box::new(Type::String)))
    }
    // M6.3 — `JSON.parse(text): T` — caller-driven type
    // inference. The return type at typecheck level is
    // Any (effectively a hole); ssa_lower's LetDecl
    // arm reads the slot's `type_ann` and emits the
    // per-shape parser at lower time. check.rs accepts
    // any `Type::Any` slot, so the let binding's
    // declared `T` slot type drives the actual decode.
    (Type::Object("JSON"), "parse") => {
        Ok(Type::Function(vec![Type::String], Box::new(Type::Any)))
    }
    // Array.isArray(x) — compile-time static check.
    (Type::Object("Array"), "isArray") => {
        Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean)))
    }
    // `Array.from(s)` over a string — returns `string[]`
    // with one single-char string per byte. The other
    // overloads (iterable / arrayLike / mapFn) aren't in
    // tr's subset; ssa_lower validates the arg is Type::Str
    // at lower-time.
    (Type::Object("Array"), "from") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Array(Box::new(Type::String))),
    )),
    // `Object.keys(obj)` — returns Array<String> with the
    // field names of obj's struct type. Static-resolved at
    // codegen (the struct layout is known at compile
    // time), so this is a compile-time constant array
    // emitted at the call site. Param is Type::Any
    // because the typechecker doesn't yet track
    // "any-struct" as a constraint; ssa_lower verifies
    // the arg actually carries Type::Obj at lower-time
    // and panics on non-struct args.
    (Type::Object("Object"), "keys")
    // tr has no prototype chain, so own == all; alias
    // getOwnPropertyNames to keys at lower time.
    | (Type::Object("Object"), "getOwnPropertyNames")
    // ES6 §28.1.11 — `Reflect.ownKeys` shares this
    // signature; the ssa_lower dispatch routes both
    // through the same struct-keys emit.
    | (Type::Object("Reflect"), "ownKeys") => Ok(Type::Function(
        vec![Type::Any],
        Box::new(Type::Array(Box::new(Type::String))),
    )),
    /* v0.2 #3 — Object.hasOwn(obj, key) — compile-time
     * resolved when key is a Str literal (struct layout
     * known at lower time). Boolean result.
     *
     * Object.freeze / isFrozen are deferred — pairing
     * them as a no-op returning false would break
     * `Object.isFrozen(Object.freeze(o)) === true`
     * test262 cases. Real implementation needs a
     * frozen bit on the universal heap header (v0.3). */
    (Type::Object("Object"), "hasOwn")
    // ES6 §28.1.9 — `Reflect.has` shares this signature.
    | (Type::Object("Reflect"), "has") => Ok(Type::Function(
        vec![Type::Any, Type::String],
        Box::new(Type::Boolean),
    )),
    // ES6 §28.1.6 — `Reflect.get(target, key)`. Subset:
    // typed struct target + literal-string key folds at
    // ssa-lower time to a struct field load + box-to-Any
    // (key not in layout → ANY_UNDEF). Dynamic key or
    // non-struct target stays a deferred substrate.
    (Type::Object("Reflect"), "get") => Ok(Type::Function(
        vec![Type::Any, Type::String],
        Box::new(Type::Any),
    )),
    // Object.is(a, b) — strict equality with two
    // corner-case overrides vs `===`: NaN is equal to
    // NaN, and +0 is NOT equal to -0. Lowered per arg
    // SSA type (Type::Number → __torajs_object_is_f64
    // runtime helper that bitcasts the ±0 case;
    // Type::String → __torajs_str_eq; everything else
    // falls back to SSA-level == compare).
    (Type::Object("Object"), "is") => Ok(Type::Function(
        vec![Type::Any, Type::Any],
        Box::new(Type::Boolean),
    )),
    /* T-09.b (v0.4.0) — Object.entries(obj) returns
     * `Array<Array<Any>>` (each inner is `[key, value]`).
     * Codegen unfolds at compile time using the static
     * struct layout from check.rs's struct_layouts —
     * zero-cost reflection just like Object.keys. The
     * Type::Any tagged-slot path from T-10 carries the
     * mixed key (Str) + value (per-field type). */
    (Type::Object("Object"), "entries") => Ok(Type::Function(
        vec![Type::Any],
        Box::new(Type::Array(Box::new(Type::Array(Box::new(Type::Any))))),
    )),
    /* T-09.c (v0.4.0) — Object.fromEntries(entries)
     * uses caller-driven typing (similar to JSON.parse):
     * the typecheck-level return is Any, and ssa_lower's
     * LetDecl arm unfolds per the slot struct schema.
     * MVP: entries are assumed to be in struct field
     * declaration order (matches Object.entries
     * round-trip), no key-matching scan. */
    (Type::Object("Object"), "fromEntries") => Ok(Type::Function(
        vec![Type::Array(Box::new(Type::Array(Box::new(Type::Any))))],
        Box::new(Type::Any),
    )),
    /* S258 — Object.values(obj) → Array<Any>. SSA-emit
     * already dispatches Obj/Arr/Str/Any receivers
     * (ssa_lower.rs ~18495); checktime sig was missing.
     * Return Array<Any> — heterogeneous struct fields
     * + Any receiver both box to Any per ssa_lower's
     * anyv_struct_values walker; homogeneous struct
     * + Arr receivers also typecheck under Array<Any>
     * (downcast-on-use). Trailing-arg widen folded
     * into S256 below (matches!("entries"|"freeze"
     * |"isFrozen"|"values"). */
    (Type::Object("Object"), "values") => Ok(Type::Function(
        vec![Type::Any],
        Box::new(Type::Array(Box::new(Type::Any))),
    )),
    /* T-09.d (v0.4.0) — Object.freeze(obj) sets the
     * FROZEN bit on the universal heap header. Returns
     * the same obj per spec. Subsequent field writes
     * are silently ignored (matches non-strict mode;
     * tr has no `"use strict"` directive). The arg
     * type is permissive (Type::Any) — runtime accepts
     * any heap object pointer. */
    (Type::Object("Object"), "freeze") => Ok(Type::Function(
        vec![Type::Any],
        Box::new(Type::Any),
    )),
    /* Object.isFrozen(obj) — reads the FROZEN bit. */
    (Type::Object("Object"), "isFrozen") => Ok(Type::Function(
        vec![Type::Any],
        Box::new(Type::Boolean),
    )),
    /* T-15.g.1 — Promise.resolve(v) / Promise.reject(v).
     * MVP only Number arg (Type::Promise<Number>);
     * heap types (Promise<string>, etc.) land in
     * T-15.g.4 via direct call-arm handling that
     * inspects the inferred arg type at the call site
     * (the static-method table's TypeVar isn't
     * instantiated automatically). */
    (Type::Object("Promise"), "resolve")
    | (Type::Object("Promise"), "reject") => Ok(Type::Function(
        vec![Type::Number],
        Box::new(Type::Promise(Box::new(Type::Number))),
    )),
    /* T-15.g.3 / T-19.g (v0.5.0) — `Promise<T>.then(cb)`
     * chains. cb signature is `(v: T) => T` (same T
     * in/out — no generic U yet). T ∈ Number / String
     * / Boolean — the three i64-roundtrippable
     * primitives the runtime helper
     * `__torajs_promise_then_simple` packs through.
     * Heap T (Array / Struct / Date) deferred to
     * T-15.g.5+ alongside the closure-cb substrate. */
    (Type::Promise(inner), "then")
        if matches!(
            **inner,
            Type::Number | Type::String | Type::Boolean | Type::Any
        ) =>
    {
        // P10.7 — `Promise<Any>` participates same as
        // the i64-roundtrippable primitives: cb is
        // `(v: Any) => Any`; the existing
        // `__torajs_promise_then_simple` helper takes
        // / returns i64 (NaN-box AnyValue at the SSA
        // layer is i64-sized).
        Ok(Type::Function(
            vec![Type::Function(
                vec![(**inner).clone()],
                Box::new((**inner).clone()),
            )],
            Box::new(Type::Promise(inner.clone())),
        ))
    }
    /* T-19.k (v0.5.0) — `Promise<T>.catch(onRejected)`.
     * cb sig is `(reason: T) => T` — same shape as
     * .then's onFulfilled. Returns a Promise<T> that
     * resolves with cb's return value on rejection,
     * or passes through source's value on fulfillment.
     * T scope matches .then (Number / String / Boolean)
     * since both share the i64-roundtripping runtime
     * helper. spec-strict heterogeneous T → U lands
     * with TypeVar substitution post-T-15.g.4. */
    (Type::Promise(inner), "catch")
        if matches!(
            **inner,
            Type::Number | Type::String | Type::Boolean | Type::Any
        ) =>
    {
        // P10.7 — symmetric with the `.then` widening
        // above. `Promise<Any>.catch(cb)` runs through
        // `__torajs_promise_catch_simple` / `_closure`
        // unchanged at the SSA layer.
        Ok(Type::Function(
            vec![Type::Function(
                vec![(**inner).clone()],
                Box::new((**inner).clone()),
            )],
            Box::new(Type::Promise(inner.clone())),
        ))
    }
    /* T-19.k — `Promise<T>.finally(onFinally)`. cb sig
     * is `() => void` per spec — no value passed in,
     * cb's return ignored. Returns a Promise<T> with
     * the same state + value as the source (after
     * cb runs). cb runs on either settled state. */
    (Type::Promise(inner), "finally") => Ok(Type::Function(
        vec![Type::Function(vec![], Box::new(Type::Void))],
        Box::new(Type::Promise(inner.clone())),
    )),
    /* T-13.b (v0.4.0) — Symbol.for(key) returns the
     * registered Symbol for the key (creates one on
     * first call). Identity preserved across calls. */
    (Type::Object("Symbol"), "for") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Symbol),
    )),
    /* Symbol.keyFor(s) — inverse: returns the key
     * Symbol.for() registered the symbol under, or
     * null for unregistered (Symbol(...)) symbols. */
    (Type::Object("Symbol"), "keyFor") => Ok(Type::Function(
        vec![Type::Symbol],
        Box::new(Type::Nullable(Box::new(Type::String))),
    )),
    /* T-13.c (v0.4.0) — well-known Symbol singletons.
     * Process-level lazy-init pointers; identity
     * preserved across all access sites. for-of
     * dispatch via `[Symbol.iterator]()` lands with
     * v0.5 (iterator protocol substrate). */
    (Type::Object("Symbol"), "iterator")
    | (Type::Object("Symbol"), "asyncIterator")
    | (Type::Object("Symbol"), "toPrimitive") => Ok(Type::Symbol),
    /* T-09.a (v0.4.0) — 5 Object methods that don't fit
     * tr's nominal class system / fixed struct schema.
     * Reject at typecheck with a clear phase pointer
     * rather than ship a silently-wrong implementation.
     *
     * - getPrototypeOf / setPrototypeOf: bun returns the
     *   prototype object (a runtime value); tr's nominal
     *   class system has no equivalent runtime concept.
     *   Lands with T-27 (Function constructor era) when
     *   dynamic substrate becomes available.
     * - defineProperty / defineProperties /
     *   getOwnPropertyDescriptor: dynamic property add /
     *   descriptor introspection requires schema
     *   mutation; tr's struct layout is fixed at class
     *   declaration. Lands with T-27 / Type::Any field
     *   substrate post-v0.5.
     */
    // P4.2 Phase B+C — Object.getPrototypeOf returns
    // the class's prototype object as an Any-box (the
    // same `__proto_<C>` registered via
    // __torajs_proto_register at module init). Pre-P4.2
    // the stub returned Null; with prototype singletons
    // now exposed, return Any so the caller can `===`
    // against `C.prototype` and chain-walk via further
    // getPrototypeOf calls. Returns ANY_NULL (still
    // Type::Any tag-wise) when the arg has no prototype
    // (Type::Obj with class_tag 0, or a Type::Any whose
    // dynobj lacks `__proto__`).
    (Type::Object("Object"), "getPrototypeOf") => {
        Ok(Type::Function(vec![Type::Any], Box::new(Type::Any)))
    }
    // P3.3 — Object.defineProperty(obj, key, descriptor)
    // accepted at typecheck. ssa_lower intercepts the
    // Call, extracts descriptor.value (other descriptor
    // fields like writable/configurable/enumerable/get/
    // set are subset-deferred), and routes to dynobj_set.
    // obj is Type::Any (must be a dynobj-backed Any-box);
    // key is Type::String; descriptor is Type::Any
    // (typically a plain object literal at the call site
    // — ssa_lower probes for the .value field at AST time).
    (Type::Object("Object"), "defineProperty") => Ok(Type::Function(
        vec![Type::Any, Type::String, Type::Any],
        Box::new(Type::Void),
    )),
    // P3.getOwnPropertyDescriptor — accept at typecheck.
    // ssa_lower intercepts and constructs an Any-boxed
    // descriptor object `{value, writable, enumerable,
    // configurable}` from the dynobj bucket's stored
    // tag/value/flags (per dcf069f attribute-flag
    // tracking). Missing key returns Any-boxed undefined.
    (Type::Object("Object"), "getOwnPropertyDescriptor") => Ok(
        Type::Function(
            vec![Type::Any, Type::String],
            Box::new(Type::Any),
        ),
    ),
    // 2026-05-18 — accept these as permissive Any
    // typecheck-only stubs (no real substrate yet).
    // ssa_lower has no special intercept either: the
    // calls reach the generic call path and would
    // panic. With test262 5k unlock being the goal,
    // accept here so harness-shim consumers (which
    // never read the return) flow through; cases
    // that need real spec behavior bucket as bugs
    // rather than incompatible.
    (Type::Object("Object"), "setPrototypeOf") => Ok(Type::Function(
        vec![Type::Any, Type::Any],
        Box::new(Type::Any),
    )),
    (Type::Object("Object"), "defineProperties") => Ok(Type::Function(
        vec![Type::Any, Type::Any],
        Box::new(Type::Void),
    )),
    // `Object.create(proto, descriptors?)` — common
    // test262 init pattern (`Object.create(null)`).
    // Returns Any (a fresh dynobj-backed Any-box at
    // lower time).
    (Type::Object("Object"), "create") => Ok(Type::Function(
        vec![Type::Any],
        Box::new(Type::Any),
    )),
    // `Object.assign(target, ...sources)` — copy own
    // enumerable props. Subset accepts any-typed
    // target + variadic any sources; ssa_lower's
    // generic-call path picks it up as a no-op
    // (returns target) if not intercepted.
    (Type::Object("Object"), "assign") => Ok(Type::Function(
        vec![Type::Any, Type::Any],
        Box::new(Type::Any),
    )),
    // `Object.preventExtensions(obj)` /
    // `Object.isExtensible(obj)` / `Object.seal(obj)`
    // / `Object.isSealed(obj)` — no-op substrate
    // returns the obj / true|false. Real semantics
    // (frozen-bit dispatch) requires runtime header
    // flag extension — deferred.
    (Type::Object("Object"), "preventExtensions")
    | (Type::Object("Object"), "seal") => Ok(Type::Function(
        vec![Type::Any],
        Box::new(Type::Any),
    )),
    (Type::Object("Object"), "isExtensible")
    | (Type::Object("Object"), "isSealed") => Ok(Type::Function(
        vec![Type::Any],
        Box::new(Type::Boolean),
    )),
    (Type::String, "length") | (Type::Array(_), "length") => Ok(Type::Number),
    /* P6.1 / P6.2 — Map.prototype.size / Set.prototype.size
     * accessor (spec §23.1.3.10 / §24.2.3.9). Member
     * arm dispatches to a Number-typed read; ssa_lower
     * calls `__torajs_map_size` (Set storage is the
     * same Map runtime). */
    // (Type::Map | Set, "size") — handled by the
    // pre-match Set/Map try_match dispatch (chunk 193).
    // M6.1 — String methods. All borrow `this` and any
    // String args (consumption only fires at concat,
    // which has its own arm). Bool-returning methods
    // return Type::Boolean; index/charCodeAt return
    // Number; slice returns String.
    (Type::String, "slice") | (Type::String, "substring") => {
        Ok(Type::Function(
            vec![Type::Number, Type::Number],
            Box::new(Type::String),
        ))
    }
    // T-49 — `String.prototype.substr(start, length?)` (annexB
    // legacy). The 1-arg shape is the common one in test262;
    // the call-site arity-tolerance arm above
    // (`slice / substring / substr` with args.len() < 2)
    // accepts 0/1 args, and ssa_lower fills the missing
    // length with i64::MAX so the runtime helper clamps.
    (Type::String, "substr") => Ok(Type::Function(
        vec![Type::Number, Type::Number],
        Box::new(Type::String),
    )),
    (Type::String, "repeat") => Ok(Type::Function(
        vec![Type::Number],
        Box::new(Type::String),
    )),
    (Type::String, "toUpperCase") | (Type::String, "toLowerCase")
    | (Type::String, "trim") | (Type::String, "trimStart")
    | (Type::String, "trimEnd")
    // `trimLeft` / `trimRight` are the non-standard but
    // de-facto aliases that ship in every JS engine —
    // ECMAScript Annex B documents them as legacy of
    // `trimStart` / `trimEnd`.
    | (Type::String, "trimLeft") | (Type::String, "trimRight")
    // s.normalize() — Unicode normalization. tr's
    // current Str layer is byte-oriented; for ASCII
    // strings (the dominant test262 case) all four NFC/
    // NFD/NFKC/NFKD forms are byte-identical with the
    // input, so an identity stub round-trips correctly.
    // Multi-byte UTF-8 strings would need Unicode tables
    // — deferred to v1.0 (`\p{...}` + ICU work).
    | (Type::String, "normalize")
    // ES2024 §22.1.3.30 — `toWellFormed()`. torajs is
    // internally UTF-8, so lone surrogates can't be
    // encoded; identity stub at lower time.
    | (Type::String, "toWellFormed") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::String),
    )),
    // ES2024 §22.1.3.10 — `isWellFormed()`. Mirror of
    // toWellFormed (see above) — always true.
    (Type::String, "isWellFormed") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::Boolean),
    )),
    (Type::String, "padStart") | (Type::String, "padEnd") => {
        Ok(Type::Function(
            vec![Type::Number, Type::String],
            Box::new(Type::String),
        ))
    }
    (Type::String, "replace") | (Type::String, "replaceAll") => {
        // Pattern arg is either a literal Str (existing
        // string-only path through __torajs_str_replace
        // / __torajs_str_replace_all) or a RegExp
        // (Phase 1b regex path). Repl arg is either a
        // Str (existing path) or a callback fn (P9.5).
        // Both args use Type::Any here so each can pass
        // typecheck; ssa_lower picks the dispatch by
        // operand SSA type. A1 callback shape required:
        // `(m: string) => string` — multi-arg / capture-
        // spread callbacks are A1.1.
        Ok(Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::String),
        ))
    }
    // `s.charAt(i)` — single-char substring at index i.
    // Identical surface to `s[i]`; routed through the
    // same substr_create / substr_slice path at lower
    // time. tr's subset doesn't return "" on OOB —
    // matches the unchecked-index convention used by
    // index access.
    (Type::String, "charAt") => Ok(Type::Function(
        vec![Type::Number],
        Box::new(Type::String),
    )),
    (Type::String, "at") => Ok(Type::Function(
        vec![Type::Number],
        Box::new(Type::String),
    )),
    // (Type::Number, "toFixed" / "toExponential" /
    // "toPrecision" / "toString" / "toLocaleString") —
    // handled by the pre-match prim try_match (chunk 194).
    // S140 — `s.toLocaleLowerCase` / `toLocaleUpperCase`
    // per ES §22.1.3.21 / §22.1.3.23 accept optional
    // locales arg; tr's subset is en-US only so the
    // arg is accepted (Any?) and ignored — same shape
    // as Number.toLocaleString (S139).
    (Type::String, "toLocaleLowerCase") | (Type::String, "toLocaleUpperCase") => {
        Ok(Type::Function(vec![Type::Any], Box::new(Type::String)))
    }
    // (Type::Boolean / BigInt / Symbol, "toString" /
    // "toLocaleString" / "valueOf") — handled by the
    // pre-match prim try_match (chunk 194).
    // V3-18 m2.c — `.constructor` on primitives
    // returns the constructor function (Number /
    // String / etc). Subset stub: Type::Any (the
    // constructor's actual type is callable but
    // tora has no first-class function reference for
    // the namespace ctor; Type::Any lets the test
    // typecheck without committing to a real shape).
    (Type::Number, "constructor")
    | (Type::String, "constructor")
    | (Type::Boolean, "constructor")
    | (Type::BigInt, "constructor")
    | (Type::Symbol, "constructor") => Ok(Type::Any),
    // V3-18 m2.a — Object.prototype methods exposed on
    // every primitive via JS's auto-boxing rules:
    //   .valueOf()              → returns the primitive itself
    //   .hasOwnProperty(name)    → false (primitives have
    //                              no own properties in our
    //                              subset)
    //   .propertyIsEnumerable(name) → false (same)
    //   .isPrototypeOf(obj)     → false (we have no real
    //                              prototype chain)
    // ssa_lower handles the dispatch with constant folds
    // since the values can't actually carry user-added
    // properties.
    // (Type::Number, "valueOf") — handled by the pre-match
    // prim try_match (chunk 194).
    // (Type::Array(_), "valueOf") — handled by the
    // pre-match Array try_match dispatch (chunk 196).
    (Type::String, "valueOf")
    // V3-18 wedge — String.prototype.toString /
    // toLocaleString / valueOf all return the
    // primitive string itchecker per JS spec
    // §22.1.3.27 / §22.1.3.31 / §22.1.3.34.
    // Already wired for Number / Boolean / BigInt /
    // Symbol but missing for String, so `s.toString()`
    // hit 'no member .toString on type String'.
    | (Type::String, "toString")
    | (Type::String, "toLocaleString") => {
        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
    }
    // (`(Type::Boolean, "valueOf")` is handled by the
    // earlier Boolean arm — dead duplicate removed for
    // the zero-warn build rule.)
    // (Type::BigInt, "valueOf") — handled by the pre-match
    // prim try_match (chunk 194).
    (Type::Number, "hasOwnProperty")
    | (Type::String, "hasOwnProperty")
    | (Type::Boolean, "hasOwnProperty")
    | (Type::BigInt, "hasOwnProperty")
    | (Type::Symbol, "hasOwnProperty")
    | (Type::Any, "hasOwnProperty")
    | (Type::Number, "propertyIsEnumerable")
    | (Type::String, "propertyIsEnumerable")
    | (Type::Boolean, "propertyIsEnumerable")
    | (Type::BigInt, "propertyIsEnumerable")
    | (Type::Symbol, "propertyIsEnumerable")
    | (Type::Any, "propertyIsEnumerable") => {
        Ok(Type::Function(vec![Type::String], Box::new(Type::Boolean)))
    }
    (Type::Any, "valueOf") => Ok(Type::Function(Vec::new(), Box::new(Type::Any))),
    (Type::Any, "toString") => Ok(Type::Function(Vec::new(), Box::new(Type::String))),
    (Type::Any, "isPrototypeOf") => Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean))),
    (Type::Any, "constructor") => Ok(Type::Any),
    // RegExp instance methods. v0.2 #1 ships `.test(s)`;
    // `.exec` / `.toString` / `.source` / `.flags` /
    // `.global` / `.lastIndex` come in subsequent
    // sub-phases. The matching engine in
    // `runtime_regex.c` is the single source of truth
    // for both `re.test(s)` and the `s.match(re)` /
    // `s.replace(re, repl)` paths in v0.2 #1.b/c.
    // (Type::RegExp, _) — handled by the pre-match
    // RegExp try_match dispatch (chunk 195).
    // (Type::WeakRef / WeakMap / WeakSet, _) — handled
    // by the pre-match try_match dispatch; see chunk 192
    // note above.
    // (Type::Map | Set | MapIter | ArrIter, _) — handled
    // by the pre-match Set/Map try_match dispatch (chunk 193).
    // (Type::Date, _) instance methods — handled by the
    // pre-match try_match dispatch; see chunk 191 note above.
    // Date.now() — static, returns ms-since-epoch.
    (Type::Object("Date"), "now") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::Number),
    )),
    /* v0.3 #2 — Bun namespace (minimum).
     * Bun.write(path, data) — bun-shape file write,
     * routes to the same fs intrinsic. Bun.file(path)
     * (chained-method shape returning a File object)
     * lands when the surface gains object-result Calls. */
    (Type::Object("Bun"), "write") => Ok(Type::Function(
        vec![Type::String, Type::String],
        Box::new(Type::Void),
    )),
    /* T-19 (v0.5.0) — `Bun.file(path)` returns an
     * opaque BunFile handle. The user calls `.text()`
     * (or future `.json()` / `.arrayBuffer()`) on it
     * to actually read. The handle is internally
     * `Type::String` (just the path) since the
     * methods all dispatch through fs.readFileSync.
     * Type::Object("BunFile") sentinel keeps the
     * methods scoped so plain Strings don't match. */
    (Type::Object("Bun"), "file") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Object("BunFile")),
    )),
    /* V3-08 — `Bun.gc(synchronous)`. tora's Bacon-Rajan
     * cycle collector triggers regardless of the bool
     * arg (we ignore it; bun uses it to gate JSC's
     * concurrent GC). Both runtimes return void. */
    (Type::Object("Bun"), "gc") => Ok(Type::Function(
        vec![Type::Boolean],
        Box::new(Type::Void),
    )),
    (Type::Object("BunFile"), "text") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::Promise(Box::new(Type::String))),
    )),
    /* T-19.c (v0.5.0) — `Bun.file(p).exists()`. Bun
     * exposes this as a fast existence-probe that
     * doesn't open the file. Maps to fs.existsSync
     * in the MVP "synchronous-then-resolve" model. */
    (Type::Object("BunFile"), "exists") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::Promise(Box::new(Type::Boolean))),
    )),
    /* T-19.d (v0.5.0) — `Bun.file(p).json()` returns
     * Promise<Any>. The actual return type comes from
     * the caller-driven `let X: T = await Bun.file(p)
     * .json()` shape detection in ssa_lower's LetDecl
     * arm — JSON.parse drives parsing per the slot's
     * concrete T (number / string / Struct / Array<T>
     * / etc.). At the typecheck layer we accept any
     * slot type as long as the JSON parser knows how
     * to handle it; concrete validation happens at
     * lower time. */
    (Type::Object("BunFile"), "json") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::Promise(Box::new(Type::Any))),
    )),
    /* T-18.c (v0.5.0) — `Bun.file(p).size` synchronous
     * property (NOT a method). Returns the file's
     * byte size, or -1 if the path is missing or
     * non-regular (bun returns 0 for missing — tr
     * uses -1 to keep the missing case observable
     * until typed-throw fs lands). */
    (Type::Object("BunFile"), "size") => Ok(Type::Number),
    /* T-21 (v0.6.0) — `fetch(url)` Response surface.
     * `.text()` returns the (already-loaded) body as
     * `Promise<string>`; `.status` is the HTTP status
     * code (0 on transport error). `.ok` and JSON
     * parsing land alongside the fetch options
     * follow-up. */
    (Type::Object("Response"), "text") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::Promise(Box::new(Type::String))),
    )),
    (Type::Object("Response"), "status") => Ok(Type::Number),
    /* v0.3 #3 — process surface (minimum). */
    (Type::Object("process"), "exit") => Ok(Type::Function(
        vec![Type::Number],
        Box::new(Type::Void),
    )),
    (Type::Object("process"), "cwd") => Ok(Type::Function(
        Vec::new(),
        Box::new(Type::String),
    )),
    /* `process.platform` — value access, not a Call.
     * Returned as Type::String; ssa_lower's Member arm
     * emits a runtime call to __torajs_process_platform. */
    (Type::Object("process"), "platform") => Ok(Type::String),
    /* `process.argv` / `Bun.argv` — runtime array of
     * argv strings. Lowered by ssa_lower's Member arm
     * to __torajs_process_argv(). */
    (Type::Object("process"), "argv")
    | (Type::Object("Bun"), "argv") => {
        Ok(Type::Array(Box::new(Type::String)))
    }
    /* `process.env` — env-namespace Object; member
     * access on it (`process.env.NAME`) routes through
     * the (Object("env"), _) arm below to runtime getenv. */
    (Type::Object("process"), "env") => Ok(Type::Object("env")),
    /* `process.env.NAME` — Nullable<String> (NULL when
     * var unset; tr's undefined→null bridge keeps
     * `=== undefined` round-tripping). */
    (Type::Object("env"), _) => Ok(Type::Nullable(Box::new(Type::String))),
    /* T-03 (v0.3.0) — process.{stdout, stderr, stdin}
     * value-Member: each exposes its own Object so the
     * downstream `.write` / `.read` Call resolves at
     * the (Object("process_stdout"), "write") arm
     * below. (`process.stdout` itchecker is also a legal
     * value reference — e.g. `let s = process.stdout`
     * — so the value-Member must be type-able too.) */
    (Type::Object("process"), "stdout") => Ok(Type::Object("process_stdout")),
    (Type::Object("process"), "stderr") => Ok(Type::Object("process_stderr")),
    /* `process.stdin` deferred — see comment on .read above. */
    /* T-03 — process.stdout / process.stderr.write(s)
     * Call shape. Returns Boolean to match bun's
     * `process.stdout.write(s)` signature (true on
     * success, false on backpressure / error — tr
     * panics on short write so it always returns true
     * when control returns). */
    (Type::Object("process_stdout"), "write")
    | (Type::Object("process_stderr"), "write") => {
        Ok(Type::Function(
            vec![Type::String],
            Box::new(Type::Boolean),
        ))
    }
    /* `process.stdin.read()` deferred to v0.5 — bun's
     * API is Node Readable async (returns Buffer-or-
     * null), so a sync drain-to-EOF would diverge from
     * the oracle. Lands with the async substrate. */

    /* v0.3 #1 — fs module surface (Phase 2.0a substrate).
     * Synchronous file I/O; throw on error is Phase 2.0b. */
    (Type::Object("fs"), "readFileSync") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::String),
    )),
    (Type::Object("fs"), "writeFileSync") => Ok(Type::Function(
        vec![Type::String, Type::String],
        Box::new(Type::Void),
    )),
    (Type::Object("fs"), "appendFileSync") => Ok(Type::Function(
        vec![Type::String, Type::String],
        Box::new(Type::Void),
    )),
    (Type::Object("fs"), "unlinkSync")
    | (Type::Object("fs"), "mkdirSync") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Void),
    )),
    (Type::Object("fs"), "existsSync") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Boolean),
    )),
    /* T-18.b (v0.5.0) — fs.readdirSync(path) returns
     * Array<string> with one entry per child (`.` /
     * `..` filtered, matching bun spec). */
    (Type::Object("fs"), "readdirSync") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Array(Box::new(Type::String))),
    )),
    (Type::Object("fs_promises"), "readdir") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Promise(Box::new(
            Type::Array(Box::new(Type::String))
        ))),
    )),
    /* T-18.a (v0.5.0) — `fs/promises` module. Each
     * method calls the matching sync helper from
     * `fs.<X>Sync` then wraps the result in
     * Promise.resolve(...). MVP "synchronous-then-
     * resolve" — real I/O suspension needs T-16
     * state-machine async/await. Bun-parity:
     * `import { readFile } from "fs/promises"; await
     * readFile(p)` yields the file contents
     * byte-identical with bun. */
    (Type::Object("fs_promises"), "readFile") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Promise(Box::new(Type::String))),
    )),
    (Type::Object("fs_promises"), "writeFile") => Ok(Type::Function(
        vec![Type::String, Type::String],
        Box::new(Type::Promise(Box::new(Type::Void))),
    )),
    (Type::Object("fs_promises"), "appendFile") => Ok(Type::Function(
        vec![Type::String, Type::String],
        Box::new(Type::Promise(Box::new(Type::Void))),
    )),
    (Type::Object("fs_promises"), "unlink")
    | (Type::Object("fs_promises"), "mkdir") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Promise(Box::new(Type::Void))),
    )),
    (Type::Object("fs_promises"), "exists") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Promise(Box::new(Type::Boolean))),
    )),
    // Phase 2.0b.2 — Date.parse(s) returns ms-since-epoch
    // (or NaN sentinel — tr returns INT64_MIN; spec is NaN).
    (Type::Object("Date"), "parse") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Number),
    )),
    // Date.UTC(year, month, day, hour, min, sec, ms) — UTC
    // interpretation; returns ms-since-epoch. Min 2 args.
    // tr accepts the 7-arg form via the same dispatch path
    // as `new Date(...)` component ctor; missing trailing
    // args default to month=0, day=1, rest=0 — but that
    // padding happens at desugar time, which doesn't
    // intercept `Date.UTC(...)` (only `new Date(...)`).
    // For Phase 2.0b.2, tr's Date.UTC requires explicit
    // 7 args; arity-aware desugar comes in 2.0c.
    (Type::Object("Date"), "UTC") => Ok(Type::Function(
        vec![Type::Number; 7],
        Box::new(Type::Number),
    )),
    // String namespace static — `String.fromCharCode(n)`.
    // `fromCodePoint` is the Unicode-aware sibling; in
    // tr's byte-Str layout the two collapse for code
    // points ≤ 0xff and ports keep arguments inside that
    // range to stay bun-portable.
    (Type::Object("String"), "fromCharCode")
    | (Type::Object("String"), "fromCodePoint") => Ok(Type::Function(
        vec![Type::Number],
        Box::new(Type::String),
    )),
    (Type::String, "charCodeAt") | (Type::String, "codePointAt") => {
        Ok(Type::Function(
            vec![Type::Number],
            Box::new(Type::Number),
        ))
    }
    (Type::String, "startsWith") | (Type::String, "endsWith")
    | (Type::String, "includes") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::Boolean),
    )),
    (Type::String, "indexOf")
    | (Type::String, "lastIndexOf")
    | (Type::String, "localeCompare")
    // V3-18 wedge — String.prototype.search per JS
    // spec §22.1.3.16. The full spec coerces the
    // arg to a RegExp and uses Symbol.search, but
    // for a plain string arg the result is exactly
    // indexOf — first match position or -1.
    // tora's subset only routes the string-arg
    // form (RegExp arg is a follow-up substrate
    // item alongside Symbol.search dispatch).
    | (Type::String, "search") => {
        Ok(Type::Function(
            vec![Type::String],
            Box::new(Type::Number),
        ))
    }
    // s.split(sep): string[] — `sep` is Str or RegExp;
    // Type::Any lets either type pass typecheck and
    // ssa_lower dispatches on operand SSA type to the
    // string-only `__torajs_str_split` or the regex
    // path `__torajs_str_split_regex`.
    // V3-18 wedge — `s.split(sep [, limit])` per ES
    // §22.1.3.21. The optional `limit` slot uses
    // Type::Any so the trailing-Any arity-pad path
    // makes 1-arg calls type-check; the SSA layer in
    // `ssa_lower_str.rs` already branches on
    // `args.len() == 2` to emit the slice clamp.
    (Type::String, "split") => Ok(Type::Function(
        vec![Type::Any, Type::Any],
        Box::new(Type::Array(Box::new(Type::String))),
    )),
    // s.match(re) — Phase 1b returns Array<Str>; without
    // `g` flag the array has 1 element (the matched
    // substring), with `g` it has all matches. Capture
    // groups + JS-spec null-on-miss are Phase 1c.
    (Type::String, "match") => Ok(Type::Function(
        vec![Type::RegExp],
        Box::new(Type::Array(Box::new(Type::String))),
    )),
    // s.matchAll(re) — Phase 1c.3 returns
    // Array<Array<Str>>: outer = one entry per match,
    // each inner = exec-shape [match, g1, g2, ...].
    // JS spec returns an iterator; tr's array stand-in
    // covers the dominant test262 usage pattern (a for-of
    // loop or [...m]) until iterator protocol lands.
    (Type::String, "matchAll") => Ok(Type::Function(
        vec![Type::RegExp],
        Box::new(Type::Array(Box::new(
            Type::Array(Box::new(Type::String))
        ))),
    )),
    // (Type::Array(_), "join" / "toString" / "toLocaleString"
    // / "push" / "pop" / "shift" / "unshift" / "splice" /
    // "toSpliced" / "flat" / "toSorted" / "sort" / "concat")
    // — handled by the pre-match Array try_match dispatch
    // (chunk 196).
    // `s.concat(other)` — string concat. The single-arg
    // shape lives here so the standard method-call path
    // typechecks normally. Variadic forms drop into the
    // arity-≠-1 guard below the Math/String variadic
    // block.
    (Type::String, "concat") => Ok(Type::Function(
        vec![Type::String],
        Box::new(Type::String),
    )),
    // (Type::Array(_), "at" / "reverse" / "toReversed" /
    // "with" / "copyWithin" / "fill" / "slice" / "indexOf"
    // / "lastIndexOf" / "map" / "flatMap" / "filter" /
    // "reduce" / "reduceRight" / "forEach" / "keys" /
    // "values" / "entries" / "includes" / "find" /
    // "findLast" / "findIndex" / "findLastIndex" / "some"
    // / "every") — handled by the pre-match Array
    // try_match dispatch (chunk 196).
    // (Type::Symbol, "description") — handled by the
    // pre-match prim try_match (chunk 194).
    // (`<prim>.constructor` — V3-18 m2.c — is handled
    // by the earlier identical arm; dead duplicate
    // removed for the zero-warn build rule.)
    // V3-18 m2.d — class-instance Object.prototype
    // methods. Same shape as namespace ctors:
    //   .hasOwnProperty(k)         → true if k is a
    //                                 declared field
    //                                 (compile-time
    //                                 layout lookup).
    //   .propertyIsEnumerable(k)   → same as
    //                                 hasOwnProperty
    //                                 (instance fields
    //                                 are enumerable).
    //   .isPrototypeOf(x)          → false (no real
    //                                 prototype chain).
    //   .valueOf()                 → identity (the
    //                                 instance).
    //   .toString()                → "[object Object]"
    //                                 (subset stub).
    //   .constructor                → Type::Any.
    (Type::Struct(_), "hasOwnProperty")
    | (Type::Struct(_), "propertyIsEnumerable") => {
        Ok(Type::Function(vec![Type::String], Box::new(Type::Boolean)))
    }
    (Type::Struct(_), "isPrototypeOf") => {
        Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean)))
    }
    (Type::Struct(_), "valueOf") => {
        let inner = obj_ty.clone();
        Ok(Type::Function(Vec::new(), Box::new(inner)))
    }
    (Type::Struct(_), "toString") => {
        Ok(Type::Function(Vec::new(), Box::new(Type::String)))
    }
    (Type::Struct(_), "constructor") => Ok(Type::Any),
    // P3.2 — Member access on Type::Any returns Type::Any.
    // Static layout unknown at compile time; ssa_lower
    // routes through dynobj_get_tag/value. Missing
    // properties read as undefined per spec.
    (Type::Any, _) => Ok(Type::Any),
    // T-29 — Array-as-Object reads. `arr.x` on an
    // array returns Type::Any (lookup via side table).
    // .length is already handled by the (Type::Array(_),
    // "length") arm above; built-in methods (map /
    // filter / push / etc.) are handled in the
    // Expr::Call arm's per-method dispatch — those
    // never reach this Member-only path because the
    // Call dispatch matches obj_ty + name BEFORE
    // calling type_of(callee). Only bare-Member
    // access (without a following call site) lands
    // here, so excluding the well-known method names
    // keeps the user-visible Function-typed semantics
    // for `let m = arr.map` patterns.
    (Type::Array(_), name) if name != "length"
        && !is_array_method_name(name) => Ok(Type::Any),
    // T-27.c — built-in `length` (Number) and `name`
    // (String) on a Function. length is the param
    // count; name is the lifted FnDecl's name. Both
    // are compile-time constants known from the fn's
    // static signature, so ssa_lower can fold them
    // without runtime dispatch.
    (Type::Function(params, _), "length") => {
        let _ = params;
        Ok(Type::Number)
    }
    (Type::Function(..), "name") => Ok(Type::String),
    // T-27 — Function-as-Object reads. Per ECMAScript
    // §10.2 functions are objects. `f.x` on a closure
    // reads from its lazy props_dynobj at offset
    // CLOSURE_PROPS_OFF; missing/unset → undefined.
    // Other built-in props (.bind, .call, .apply,
    // .toString, etc.) are L3b T-27.c-rest — not
    // implemented; currently return undefined.
    (Type::Function(..), _) => Ok(Type::Any),
    _ => Err(format!("no member `.{name}` on type {obj_ty:?}")),
}
}
