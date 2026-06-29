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
    // `Type::String` instance methods — see
    // [`crate::check_type_of_member_string`] (chunk 197 —
    // seventh sub-batch). Mixed-type arms (the shared
    // `length` arm, prim-union `constructor`, and
    // prim+Any union `hasOwnProperty` /
    // `propertyIsEnumerable`) stay in the main match.
    if matches!(&obj_ty, Type::String)
        && let Some(r) = crate::check_type_of_member_string::try_match(&obj_ty, name)
    {
        return r;
    }
    // `Type::Promise` instance methods — see
    // [`crate::check_type_of_member_promise`] (chunk 198 —
    // eighth sub-batch). The pre-match structural getter
    // forwarding (`if let Type::Promise(inner) = &obj_ty`)
    // stays in the main module; only the 3 method arms
    // (`then` / `catch` / `finally`) move.
    if matches!(&obj_ty, Type::Promise(_))
        && let Some(r) = crate::check_type_of_member_promise::try_match(&obj_ty, name)
    {
        return r;
    }
    // `Type::Struct` / `Type::Function` / `Type::Any`
    // single-type members + per-type catch-alls — see
    // [`crate::check_type_of_member_misc`] (chunk 199 —
    // ninth sub-batch). The mixed-type prim+Any
    // `hasOwnProperty` / `propertyIsEnumerable` arm and
    // the `Array(_), name` catch-all stay in the main
    // match (their patterns aren't single-type).
    if matches!(&obj_ty, Type::Struct(_) | Type::Function(..) | Type::Any)
        && let Some(r) = crate::check_type_of_member_misc::try_match(&obj_ty, name)
    {
        return r;
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
    if let Type::Object(_) = &obj_ty
        && let Some(r) = crate::check_type_of_member_namespace::try_match(&obj_ty, name)
    {
        return r;
    }
    // I/O-flavored namespaces — see
    // [`crate::check_type_of_member_namespace_io`] (chunk
    // 201 — eleventh sub-batch). Sibling to chunk 200;
    // covers Date / Bun / BunFile / Response / process /
    // env / process_stdout-stderr / fs / fs_promises /
    // String static namespace tags.
    if let Type::Object(_) = &obj_ty
        && let Some(r) = crate::check_type_of_member_namespace_io::try_match(&obj_ty, name)
    {
        return r;
    }
    match (&obj_ty, name) {
    // (Type::Object("console" / "Math" / "Number" / "BigInt"),
    // various) — handled by the pre-match namespace
    // try_match dispatch (chunk 200).
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
    // (Type::Object("JSON" / "Array"), "stringify" / "parse" /
    // "isArray" / "from") — handled by the pre-match
    // namespace try_match dispatch (chunk 200).
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
    // (Type::Object("Object"), "is" / "entries" / "fromEntries"
    // / "values" / "freeze" / "isFrozen") — handled by the
    // pre-match namespace try_match dispatch (chunk 200).
    // (Type::Object("Promise"), "resolve" / "reject") +
    // (Type::Object("Symbol"), "for" / "keyFor") — handled
    // by the pre-match namespace try_match dispatch
    // (chunk 200).
    // (Type::Promise(_), "then" / "catch" / "finally")
    // — handled by the pre-match Promise try_match
    // dispatch (chunk 198).
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
    // (Type::String, "slice" / "substring" / "substr" /
    // "repeat" / "toUpperCase" / "toLowerCase" / "trim" /
    // "trimStart" / "trimEnd" / "trimLeft" / "trimRight" /
    // "normalize" / "toWellFormed" / "isWellFormed" /
    // "padStart" / "padEnd" / "replace" / "replaceAll" /
    // "charAt" / "at" / "toLocaleLowerCase" /
    // "toLocaleUpperCase") — handled by the pre-match
    // String try_match dispatch (chunk 197).
    // (Type::Number, "toFixed" / "toExponential" /
    // "toPrecision" / "toString" / "toLocaleString") —
    // handled by the pre-match prim try_match (chunk 194).
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
    // (Type::String, "valueOf" / "toString" / "toLocaleString")
    // — handled by the pre-match String try_match dispatch
    // (chunk 197).
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
    // (Type::Any, "valueOf" / "toString" / "isPrototypeOf"
    // / "constructor") — handled by the pre-match misc
    // try_match dispatch (chunk 199).
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
    // (Type::Object("Date" / "Bun" / "BunFile" / "Response"
    // / "process" / "env" / "process_stdout" /
    // "process_stderr" / "fs" / "fs_promises" / "String"),
    // various) — handled by the pre-match namespace_io
    // try_match dispatch (chunk 201).
    // (Type::String, "charCodeAt" / "codePointAt" /
    // "startsWith" / "endsWith" / "includes" / "indexOf" /
    // "lastIndexOf" / "localeCompare" / "search" / "split" /
    // "match" / "matchAll" / "concat") — handled by the
    // pre-match String try_match dispatch (chunk 197).
    // (Type::Array(_), "join" / "toString" / "toLocaleString"
    // / "push" / "pop" / "shift" / "unshift" / "splice" /
    // "toSpliced" / "flat" / "toSorted" / "sort" / "concat")
    // — handled by the pre-match Array try_match dispatch
    // (chunk 196).
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
    // (Type::Struct(_), "hasOwnProperty" / "propertyIsEnumerable"
    // / "isPrototypeOf" / "valueOf" / "toString" /
    // "constructor") + (Type::Any, _) catch-all —
    // handled by the pre-match misc try_match dispatch
    // (chunk 199).
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
    // (Type::Function(..), "length" / "name" + catch-all)
    // — handled by the pre-match misc try_match dispatch
    // (chunk 199).
    _ => Err(format!("no member `.{name}` on type {obj_ty:?}")),
}
}
