//! `Expr::Index` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Index` arm
//! as chunk-312 of the type_of_inner decomp.
//!
//! Rules:
//! - A NUMBER key is legal on every receiver. Which other keys are
//!   legal is [`reject_bad_key`]; §7.1.19 makes every value a
//!   property key, so for every receiver that has a property
//!   surface the answer is now the whole domain. (It was once
//!   "number only, except an `any` receiver" — V3-18's narrow —
//!   and the widening took rotations 346 through 610, one receiver
//!   at a time, each waiting on a lowering lane rather than on the
//!   rule.)
//! - What the read ANSWERS is the other half. `Type::Array(T)`
//!   indexed by a number → `T`; `Type::String` by a number →
//!   `Type::String` (§6.1.4: a length-1 string). Every non-number
//!   key answers `Type::Any`, because it can name an element, a
//!   property or a miss and the three have no common narrower
//!   type.
//! - A receiver with no property surface (a Nullable) stays on
//!   numbers alone; anything else → error.

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
        Type::String if string_key_domain(&idx_ty) => Ok(Type::Any),
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
    // A NUMBER key never reaches this match — the guard below lets
    // it past for every receiver, because whether it names an
    // element or a property is the LANE's question, not the gate's.
    // What the match answers is the other half: which non-number
    // keys this receiver takes. §7.1.19 makes every value a property
    // key, so the answer is now the whole domain everywhere except a
    // STRING receiver (see the note under the match).
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
        Type::String => string_key_domain(&idx_ty),
        // `m[Symbol.iterator]()` is an ordinary property read off the
        // prototype surface, with no element lane to collide with —
        // and for RegExp the same shape (r289; §22.2.6's well-known-
        // symbol protocol probes are all of it). Since none of them
        // carries indexed elements, a number key is just another
        // spelling and `m[0]` answers undefined like any other miss.
        Type::Map | Type::Set | Type::RegExp => coerced_key_domain(&idx_ty),
        // A NAMESPACE receiver (the hardcoded global stand-ins: Math
        // / JSON / Reflect / console …) joins for the same §7.1.19
        // reason, and load-bearingly since rotation 382 gave those
        // objects real `@@toStringTag` entries — `Math[Symbol
        // .toStringTag]` reads a property that exists. A namespace
        // has no indexed elements either, so `Math[0]` is a miss and
        // not a reject.
        Type::Object(_) => coerced_key_domain(&idx_ty),
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
    // Every receiver with a property surface now takes the whole
    // domain. Each still says so on its own arm because the arms
    // carry nine different REASONS for one shared answer, and the
    // reasons are what a widening has to be checked against — the
    // one arm that does not join (`_`, a Nullable receiver) is
    // deliberately narrow and would be lost in a merge.
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
///
/// And it is the WHOLE non-number domain, coerced keys included.
/// `x[true] = 1` stores under "true" and leaves `x[1]` undefined and
/// `length` at 0 — §7.1.19 spells the key first and only then asks
/// whether the spelling is a canonical array index, so a boolean, a
/// null or an object key never reaches the element lane at all.
/// Rotation 608 tried this as a one-word change and measured
/// `[1, 2][true]` answering 2; what refused it was not the runtime
/// but the lane list above, which sent the key down the element path
/// where a boolean zero-extends to index 1. The keyed kernel has
/// spelled all of these correctly the whole time, which is why the
/// `any` spelling of every one of them already worked.
pub(crate) fn array_key_domain(idx_ty: &Type) -> bool {
    coerced_key_domain(idx_ty)
}

/// The keys a STRING receiver takes besides a number — read by the
/// gate, by the read answer and by the character lane in
/// `lower_str_recv_index`, so they cannot drift apart.
///
/// They already had, the same way the array's had: the gate admitted
/// `Undefined` and the lane spelled its own `Any | String | Symbol`,
/// so `"ab"[undefined]` type-checked and then died coercing a Ptr to
/// i64 on a program every engine answers `undefined` for. The sixth
/// mirror of that domain, and the last one.
///
/// And it is the WHOLE domain, coerced keys included. A string was
/// the one receiver left on the spelled list, registered as a real
/// exception: it owns an element lane (`s[0]` is a character) that
/// the coerced keys would have to be told apart from at runtime,
/// which looked like a question the array lane answers and the
/// string lane does not. Measured, the string lane answers it too —
/// `("abc" as any)[[1]]` and `("abc" as any)[1n]` both hand back the
/// CHARACTER `b`, because §7.1.19 spells the key before §10.4.3 asks
/// whether the spelling is a canonical index, and the keyed kernel
/// has done both halves the whole time. What refused those keys was
/// this predicate, not the runtime — which is why the `any` spelling
/// of every one of them already worked, exactly as it did for the
/// array in rotation 609.
pub(crate) fn string_key_domain(idx_ty: &Type) -> bool {
    coerced_key_domain(idx_ty)
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
