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
        && let Some(key) = crate::ast::literal_prop_key(ast, index)
        && let Some(name) = key.as_str()
    {
        // Index reads are never the desugar's default-guarded pattern
        // load (that lane mints Member exprs) — no lenient miss here.
        // A key with a lone surrogate is not a `&str` and takes the
        // dynamic-index lane below (557-02).
        return crate::check_type_of_member::check(checker, ast, eid, &obj, name, false);
    }
    let idx_ty = checker.type_of(ast, index)?;
    reject_bad_key(obj_ty.clone(), idx_ty.clone())?;
    match obj_ty {
        // See the note in `reject_bad_key` — the three outcomes have
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
        // See the note in `reject_bad_key`.
        Type::Array(_) if array_key_domain(&idx_ty) => Ok(Type::Any),
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
        // See the Map/Set/RegExp/namespace notes in `reject_bad_key` —
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

/// §7.1.19 — whether `idx_ty` is a legal key for `obj_ty`, or the
/// loud reject. Split out of `check` at the 200-line rule when the
/// function-key admit landed (rotation 608); the caller keeps the
/// other half of the question, which is what the read ANSWERS.
///
/// The two halves have to be read together: every receiver arm
/// admitted here has a matching arm below that answers `Any`,
/// because a non-number key on those receivers can be an element,
/// a property or a miss and the three have no common narrower
/// type.
fn reject_bad_key(obj_ty: Type, idx_ty: Type) -> Result<(), String> {
    // Two domains, and which one a receiver takes is the whole
    // question. §7.1.19 makes every value a property key, but only a
    // receiver that already answers `Any` for a non-number key can
    // take the ones the RUNTIME has to coerce (a struct or a function
    // key runs OrdinaryToPrimitive through the keyed kernel); the
    // others admit the keys whose spelling is decidable here.
    //
    // Was a six-armed `&&` chain that spelled its two domains six
    // times between them. The arms are mutually exclusive on the
    // receiver, so a match says the same thing once — which is what
    // made the function-key admit a one-line change instead of six.
    let admitted = match obj_ty {
        Type::Any | Type::Struct(_) => coerced_key_domain(&idx_ty),
        // `a[k]` is an element read, a property read, or a miss, and
        // §7.1.19 decides which — from k's runtime tag for an `any`
        // key, from its spelling for a string one (`a["length"]` ≡
        // `a.length`, `a["0"]` is an element), uncoerced for a symbol
        // (step 2). The three have no common narrower type, which is
        // why the answer below is Any.
        Type::Array(_) => array_key_domain(&idx_ty),
        // §10.4.3 decides a string receiver's outcome the same three
        // ways: `s["0"]` is the character, `s["length"]` ≡ `s.length`,
        // anything else reads through to the method surface or a miss.
        // Only the NUMBER key keeps the narrow String answer below.
        Type::String => spelled_key_domain(&idx_ty),
        // `m[Symbol.iterator]()` is an ordinary property read off the
        // prototype surface, with no element lane to collide with —
        // and for RegExp the same shape (r289; §22.2.6's well-known-
        // symbol protocol probes are all of it). A NUMBER key stays
        // the loud reject on both: neither carries indexed elements.
        Type::Map | Type::Set | Type::RegExp => spelled_key_domain(&idx_ty),
        // A NAMESPACE receiver (the hardcoded global stand-ins: Math
        // / JSON / Reflect / console …) joins for the same §7.1.19
        // reason, and load-bearingly since rotation 382 gave those
        // objects real `@@toStringTag` entries — `Math[Symbol
        // .toStringTag]` reads a property that exists. A NUMBER key
        // stays the loud reject; a namespace has no indexed elements.
        Type::Object(_) => spelled_key_domain(&idx_ty),
        // A FUNCTION receiver takes the COERCED domain, the same one
        // an `any` or struct receiver takes and for the same reason:
        // §20.2 makes a function an ordinary object with an ordinary
        // property surface, `f[Symbol.hasInstance]` reads a property
        // that exists on it, and a function has no element lane for
        // §7.1.19's spelling to be told apart from — a numeric key is
        // already just the stringified expando probe this arm has
        // admitted since rotation 408. The lowering boxes the
        // receiver and rides the any lane, which coerces off the
        // key's runtime tag (rotation 609).
        Type::Function(..) => coerced_key_domain(&idx_ty),
        _ => false,
    };
    // MEASURED, not assumed: rotation 608 tried handing these five
    // receivers the coerced domain too, one word each, and the data
    // refused it. `[1, 2][true]` answered 2 — the boolean reached the
    // ELEMENT lane as index 1, where §7.1.19 spells it "true" and the
    // answer is undefined — and an object or function key on an array
    // died in the lowering coercing a heap cell to i64. These
    // receivers own an element lane that has to be told apart from
    // the property lane at runtime, which the keyed kernel does for
    // the spelled keys and not yet for the coerced ones. Registered.
    if idx_ty != Type::Number && !admitted {
        return Err(format!("index must be number, got {idx_ty:?}"));
    }
    Ok(())
}

/// The keys an `any` or STRUCT receiver takes. Everything
/// [`spelled_key_domain`] takes, plus the two the runtime has to
/// coerce: §7.1.19 runs OrdinaryToPrimitive over a STRUCT key
/// (cluster #4, rotation 438 — an object literal carrying its own
/// `toString`) and over a FUNCTION key (rotation 608 — §20.2.3.5
/// answers with the source text, which is what a computed property
/// name stored under). Both ride the keyed kernel's runtime
/// dispatch; the lowering boxes them at the lane boundary.
pub(crate) fn coerced_key_domain(idx_ty: &Type) -> bool {
    spelled_key_domain(idx_ty)
        || matches!(
            idx_ty,
            Type::Struct(_)
                | Type::Function(..)
                | Type::ClassRef(_)
                | Type::Boolean
                | Type::Null
                | Type::RegExp
                | Type::BigInt
                | Type::Array(_)
                | Type::Map
                | Type::Set
        )
}

/// Which of those the LOWERING has to box before the keyed kernel —
/// everything in the coerced domain whose SSA value is not already an
/// `AnyValue`. `Any` already is one, and `String` / `Symbol` keys have
/// dedicated lanes of their own.
///
/// Derived rather than listed, because it used to be listed: the same
/// domain was spelled once here and four more times across the
/// lowering's lane-selection sites, and every widening had to find all
/// five. Rotation 608's function key found four of them and shipped a
/// program that type-checked and then died coercing a Closure to i64.
pub(crate) fn key_needs_boxing(idx_ty: &Type) -> bool {
    coerced_key_domain(idx_ty) && !matches!(idx_ty, Type::Any | Type::String | Type::Symbol)
}

/// Whether a key rides the keyed kernel's runtime §7.1.19 dispatch
/// rather than a static lane — the boxed keys plus `Any`, which is
/// already boxed and can be any of them at runtime.
pub(crate) fn key_rides_keyed_kernel(idx_ty: &Type) -> bool {
    matches!(idx_ty, Type::Any) || key_needs_boxing(idx_ty)
}

/// The keys an ARRAY receiver takes besides a number — the one place
/// the gate, the read answer and the two lowering lanes all read it
/// from, so they cannot drift apart.
///
/// They already had. The gate admitted `Undefined`; the read lane in
/// `ssa_lower_index` spelled `Any | String | Symbol` and did not. So
/// `x[undefined]` type-checked and then died in the lowering
/// ("cannot coerce Ptr to i64") on a program every engine answers
/// `undefined` for — the fifth mirror of a domain that already had
/// four, and the one `key_needs_boxing` did not reach because it
/// asks a different question (what to box, not which lane).
pub(crate) fn array_key_domain(idx_ty: &Type) -> bool {
    spelled_key_domain(idx_ty)
}

/// The keys every property-key receiver takes.
///
/// A STRING key is the §7.1.19 identity (`o["k"]` is `o.k`), a SYMBOL
/// is the other half of the key domain and §7.1.19 step 2 hands it
/// on uncoerced, an ANY key can be any of those at runtime so the
/// keyed kernel dispatches, and UNDEFINED is the fixed string key
/// "undefined" (rotation 346). `Void` stays out — a Void expr has no
/// SSA value to box.
pub(crate) fn spelled_key_domain(idx_ty: &Type) -> bool {
    matches!(
        idx_ty,
        Type::String | Type::Symbol | Type::Any | Type::Undefined
    )
}
