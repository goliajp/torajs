//! `Expr::Index` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Index` arm
//! as chunk-312 of the type_of_inner decomp.
//!
//! Rules:
//! - Index expression `index` must typecheck to `Type::Number`
//!   (V3-18 index-expression spec narrow; non-number index is a
//!   typecheck error rather than a silent runtime coercion) —
//!   except a `Type::Any` receiver, which also admits string and
//!   symbol keys (L3b #13; ES ToPropertyKey / §6.1.7).
//! - `Type::String` indexed → `Type::String` (1-char substring;
//!   spec §6.1.4 string-indexed access returns a length-1 string).
//! - `Type::Array(T)` indexed → element type `T`.
//! - `Type::Any` indexed → `Type::Any` (TS `any` admits every
//!   member/index access; runtime dispatch via
//!   `__torajs_any_index_get` — Any-dynamic-access RFC 20260704 S3).
//! - Any other receiver type → error.

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
) -> Result<Type, String> {
    // RFC 20260715-nominal-class-identity — indexing is a structural
    // operation; a class instance types as its NAME, so unwrap it to
    // the shape first. (The literal-key lane below re-enters the member
    // checker with the receiver EXPR, which sees the name again and
    // keeps accessor / method lookup nominal.)
    let raw_obj_ty = checker.type_of(ast, obj)?;
    let obj_ty = crate::check::resolve_class_ref(
        &raw_obj_ty,
        &checker.class_structs,
        &checker.aliases,
        &checker.generic_alias_decls,
    );
    // Chunk 745 — struct receiver + compile-time literal index:
    // `g[0]` ≡ `g."0"` per ES ToPropertyKey (§7.1.19). Numeric keys
    // in object literals are stored under their integer spelling
    // (P0.10), so a literal index resolves statically through the
    // member checker (field lookup / accessor / visibility). Dynamic
    // indices on a struct keep the loud reject below (recorded
    // boundary — runtime property lookup on typed structs).
    if matches!(obj_ty, Type::Struct(_))
        && let Some(name) = crate::ast::literal_prop_key(ast, index)
    {
        // Index reads are never the desugar's default-guarded pattern
        // load (that lane mints Member exprs) — no lenient miss here.
        return crate::check_type_of_member::check(checker, ast, eid, &obj, &name, false);
    }
    let idx_ty = checker.type_of(ast, index)?;
    // L3b #13 — an `any` receiver admits string keys (ES
    // ToPropertyKey: `o["k"]` ≡ `o.k`); every other receiver keeps
    // the number-only spec narrow. Chunk 753 — a struct receiver
    // admits them too (dynamic key rides the any-index runtime lane).
    // Symbol keys ride the same two receivers: §6.1.7 makes them the
    // other half of the property-key domain, and §7.1.19 step 2 hands
    // them to the lookup uncoerced. Cluster #1 blades 3/5 — an `any`
    // KEY rides the runtime keyed kernel's ToPropertyKey dispatch on
    // both receivers (a struct receiver boxes at the lane boundary,
    // same as its str/symbol-key path).
    // Rotation 346 — an UNDEFINED key joins every receiver already in
    // the property-key domain: §7.1.19 ToPropertyKey(undefined) is
    // the string key "undefined", and the boxed undefined rides the
    // same runtime dispatch the `any` key does (the t262 dstr
    // harness's `{}[thrower()]` shape, where the throwing callee
    // types its result Undefined). `Void` stays out — a Void expr
    // has no SSA value to box.
    // Cluster #4 (rotation 438) — a STRUCT-typed key joins the any /
    // struct receivers: §7.1.19 ToPropertyKey coerces an object key
    // through its own toString/valueOf (OrdinaryToPrimitive), which
    // only the runtime can run. The lowering boxes the key and rides
    // the keyed kernel's runtime dispatch (`{}[{toString(){...}}]`,
    // the t262 target-member-computed-reference shape).
    if idx_ty != Type::Number
        && !(matches!(obj_ty, Type::Any | Type::Struct(_))
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined | Type::Struct(_)
            ))
        // An ARRAY receiver takes the same key domain the `any` and
        // struct receivers do, and for the same reason: `a[k]` is an
        // element read, a property read, or a miss, and §7.1.19
        // decides which — from k's runtime tag for an `any` key, from
        // its spelling for a string one (`a["length"]` ≡ `a.length`,
        // `a["0"]` is an element), and uncoerced for a symbol (step
        // 2). The static answer is Any for the same reason the struct
        // receiver's is: the three outcomes have no common narrower
        // type.
        && !(matches!(obj_ty, Type::Array(_))
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined
            ))
        // A STRING receiver takes that same key domain, and §10.4.3
        // decides the outcome the same three ways: `s["0"]` is the
        // character, `s["length"]` ≡ `s.length`, and anything else
        // reads through to the method surface or a miss. Only the
        // number key keeps the narrow String answer below.
        && !(matches!(obj_ty, Type::String)
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined
            ))
        // Map / Set receivers join the property-key domain for the
        // same §7.1.19 reason — `m[Symbol.iterator]()` is an ordinary
        // property read off the prototype surface (no element lane to
        // collide with; a NUMBER key stays the loud reject below,
        // since Map/Set carry no indexed elements at all).
        && !(matches!(obj_ty, Type::Map | Type::Set)
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined
            ))
        // A REGEXP receiver joins for the same §7.1.19 reason as
        // Map/Set — `re[Symbol.match]` is an ordinary property read
        // off the prototype surface (r289; the well-known-symbol
        // protocol probes of §22.2.6 all take this shape). A NUMBER
        // key stays the loud reject: a RegExp carries no indexed
        // elements.
        && !(matches!(obj_ty, Type::RegExp)
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined
            ))
        // A NAMESPACE receiver (`Type::Object` — the hardcoded global
        // stand-ins: Math / JSON / Reflect / console …) joins for the
        // same §7.1.19 reason, and it is now load-bearing: rotation
        // 382 gave those objects real `@@toStringTag` entries, so
        // `Math[Symbol.toStringTag]` reads a property that exists.
        // A NUMBER key stays the loud reject — a namespace carries no
        // indexed elements.
        && !(matches!(obj_ty, Type::Object(_))
            && matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined
            ))
    {
        return Err(format!("index must be number, got {idx_ty:?}"));
    }
    match obj_ty {
        // See the note on the reject above — the three outcomes have
        // no common narrower type, so a non-number key answers Any.
        Type::String
            if matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined
            ) =>
        {
            Ok(Type::Any)
        }
        Type::String => Ok(Type::String),
        // See the note on the reject above.
        Type::Array(_)
            if matches!(
                idx_ty,
                Type::String | Type::Symbol | Type::Any | Type::Undefined
            ) =>
        {
            Ok(Type::Any)
        }
        // An element read must answer with the element's own type,
        // CLASS AND ALL. Resolving above is about the RECEIVER — a
        // class instance indexes through its struct shape — but
        // `resolve_class_ref` recurses into the element type as well,
        // so `bs[0]` on an `Array(ClassRef("Box"))` came back as the
        // anonymous struct. Reading it straight through still worked
        // (the call checker recovers the class from the receiver
        // expression), but BINDING it did not: `const first = bs[0]`
        // recorded the nameless struct, and every method call on
        // `first` then failed to compile — "no member `.get` on type
        // Struct([(\"v\", Number)])" on a program every engine runs.
        // The mirror of the same question one level out (an element
        // of a `Promise<T>[]` is as much a promise as a binding is).
        Type::Array(_) => match &raw_obj_ty {
            Type::Array(raw_elem) => Ok((**raw_elem).clone()),
            _ => match obj_ty {
                Type::Array(elem) => Ok(*elem),
                _ => unreachable!("matched Array above"),
            },
        },
        // Chunk 753 — struct receiver + dynamic index (the literal
        // form resolved through the member checker above): runtime
        // property lookup over the struct's layout table via the
        // any-index lane; field types are heterogeneous so the
        // static answer is Any (miss → undefined). Dynamic WRITES
        // keep the loud reject in `check_assign_target` (a typed
        // slot store needs a static field type — recorded boundary).
        Type::Struct(_) => Ok(Type::Any),
        // See the Map/Set/RegExp/namespace notes on the reject above —
        // property reads over the prototype surface answer Any.
        Type::Map | Type::Set | Type::RegExp | Type::Object(_) => Ok(Type::Any),
        // A FUNCTION receiver's numeric read is the §7.1.19
        // stringified expando probe on the closure cell (the write
        // half's admit in `check_assign_target`) — heterogeneous, so
        // Any (miss → undefined).
        Type::Function(..) => Ok(Type::Any),
        // RC-4 F1a — un-narrowed Nullable<Array<T>> (exec/match
        // result) decays for indexing; null is a runtime
        // TypeError at the lowering-side guard.
        Type::Nullable(inner) if matches!(*inner, Type::Array(_)) => match *inner {
            Type::Array(elem) => Ok(*elem),
            _ => unreachable!(),
        },
        Type::Any => Ok(Type::Any),
        other => Err(format!("can't index into {other:?}")),
    }
}
