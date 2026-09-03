//! `Type::Array` instance-method arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 196 — sixth sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 191-195 try_match shape).
//!
//! Covers the contiguous block of `(Type::Array(elem), …)` arms
//! that live together as the typed-Array method table (29 arms
//! covering push / pop / shift / unshift / splice / flat /
//! sort / concat / at / reverse / with / copyWithin / fill /
//! slice / indexOf / map / flatMap / filter / reduce / forEach /
//! keys / values / entries / includes / find / findIndex /
//! some / every / join / toString and the array-only `valueOf`).
//!
//! The `(Type::String, "concat")` arm that sits between the
//! array `concat` and array `at` arms in the original file is
//! **not** Array-shaped — it stays in the main match.
//!
//! `.length` (`Type::Array(_), "length"` shared with
//! `Type::String, "length"`) stays in the main match because
//! its pattern is a `String | Array` union.
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match.

use crate::check::{Type, is_array_method_name};

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = try_match_base(obj_ty, name).or_else(|| try_match_iter(obj_ty, name))?;
    Some(Ok(ty))
}

/// First half of the typed-Array method table — identity /
/// stringify / mutation / slice / scan arms (valueOf → indexOf),
/// verbatim order from the pre-split match.
fn try_match_base(obj_ty: &Type, name: &str) -> Option<Type> {
    let ty = match (obj_ty, name) {
        // ES §23.1.3.34 — `arr.valueOf()` returns the
        // Array itchecker (identity). The default Object
        // protocol applies — Array doesn't override
        // valueOf with its own coercion. ssa_lower folds
        // the call to the receiver operand without a
        // runtime helper.
        (Type::Array(elem), "valueOf") => {
            Type::Function(Vec::new(), Box::new(Type::Array(elem.clone())))
        }
        // arr.join(sep): string. sep borrowed; result freshly
        // allocated.
        //
        // These two used to require T = String / Number / Boolean /
        // Any, which spelled out the element types the `arr_join_*`
        // kernels could read. That is a fact about the fast path, not
        // about the language: §23.1.3.18 joins an array of ANY
        // element type, and `String(arr)` — the same operation
        // through a different door — never asked. So
        // `String([[1],[2]])` compiled and `[[1],[2]].join(",")` was
        // a type error, on consecutive lines of one program. The
        // lowering takes the any lane for the element types with no
        // kernel (`ssa_lower_str_arr_join_flat`), so the question the
        // checker was answering no longer exists here.
        (Type::Array(_), "join") => Type::Function(vec![Type::String], Box::new(Type::String)),
        // V3-18 wedge — Array.prototype.toString. Per JS
        // spec §22.1.3.30, equivalent to `arr.join(",")`.
        (Type::Array(_), "toString" | "toLocaleString") => {
            Type::Function(Vec::new(), Box::new(Type::String))
        }
        // M1.2 — `xs.push(v)`: takes one element-typed arg,
        // returns the new length per JS spec §22.1.3.20.
        // Runtime helper `__torajs_arr_push` already writes
        // `len + 1` into `arr[#8]`; ssa_lower materializes
        // the ret as `Load(I64, new_arr, ARR_LEN_OFF)`.
        (Type::Array(elem), "push") => {
            Type::Function(vec![(**elem).clone()], Box::new(Type::Number))
        }
        // `xs.pop()` — remove and return the last element.
        // Mutates the receiver. tr's subset assumes a non-empty
        // array (matches the `xs[xs.length - 1]` style call
        // patterns this enables); `pop` on an empty array is
        // unchecked. Returns the element type directly (no
        // `T | undefined` since tr lacks union types).
        (Type::Array(elem), "pop") => Type::Function(Vec::new(), Box::new((**elem).clone())),
        // `xs.shift()` — same shape as pop but removes the
        // first element (memmoves the rest left). Subset
        // convention: empty-array shift is unchecked.
        (Type::Array(elem), "shift") => Type::Function(Vec::new(), Box::new((**elem).clone())),
        // `xs.unshift(v)` — insert v at slot 0 (memmoves
        // the rest right; may realloc). Returns the new
        // length per JS spec §22.1.3.34, mirroring push.
        (Type::Array(elem), "unshift") => {
            Type::Function(vec![(**elem).clone()], Box::new(Type::Number))
        }
        // `xs.splice(start, deleteCount)` — remove a slice
        // in-place + return removed slice as a fresh
        // Array<T>. Per JS spec §23.1.3.31. v0 subset: no
        // `...items` rest-arg insert form (deferred).
        (Type::Array(elem), "splice" | "toSpliced") => Type::Function(
            vec![Type::Number, Type::Number],
            Box::new(Type::Array(Box::new((**elem).clone()))),
        ),
        // `xs.flat()` — single-level flatten. Receiver must
        // be `T[][]`; result is `T[]`. v0 supports depth=1
        // only (no `.flat(2)` arg).
        (Type::Array(elem), "flat") => {
            // S129-3 Array<Any>.flat — Any elem bypasses
            // the typed Array<Array<T>> shape check: any
            // outer slot can wrap an inner Array<Any>
            // (or a scalar that passes through). Routes
            // to `__torajs_arr_flat_any` at ssa-lower
            // which decodes each slot's NaN-box tag.
            // Result stays Array<Any> (depth=1 only —
            // mirror typed flat's v0 limit).
            if matches!(**elem, Type::Any) {
                Type::Function(Vec::new(), Box::new(Type::Array(Box::new(Type::Any))))
            } else {
                // ES §23.1.3.11 — non-nested receiver returns
                // a shallow copy with the same element type
                // (depth=1 leaves non-Array slots untouched).
                // Pre-fix tora forced Array<Array<T>>, blocking
                // the spec-canonical `[1,2,3].flat()` shape.
                let result_inner = match (**elem).clone() {
                    Type::Array(inner) => *inner,
                    other => other,
                };
                Type::Function(Vec::new(), Box::new(Type::Array(Box::new(result_inner))))
            }
        }
        // `xs.sort(cmp)` — in-place sort using the comparator
        // `(a: T, b: T) => number`. Returns the same array
        // (chainable). Subset requires the comparator (no
        // default lex-sort fallback). `toSorted` (ES2023)
        // is the non-mutating sibling — identical signature,
        // fresh `Array<T>` result.
        (Type::Array(elem), "toSorted" | "sort") => {
            let inner = (**elem).clone();
            Type::Function(
                vec![Type::Function(
                    vec![inner.clone(), inner.clone()],
                    Box::new(Type::Number),
                )],
                Box::new(Type::Array(Box::new(inner))),
            )
        }
        // `a.concat(b)` — fresh array of a's elements then b's.
        // Subset: binary only, both arrays must share element type.
        (Type::Array(elem), "concat") => {
            // S129-4 Array<Any>.concat — Any receiver accepts
            // any Array<U> arg (typed slots get NaN-boxed at
            // runtime via __torajs_arr_extend_typed_into_any
            // when the SSA arg type isn't Array<Any>).
            // Param sig is Type::Any so dispatch typecheck
            // doesn't reject typed Array<U>; ssa-lower peeks
            // expr_types to derive the elem tag. Result
            // stays Array<Any>. Same S128-5 / S129-1 / S129-3
            // mixed-Any series shape.
            if matches!(**elem, Type::Any) {
                Type::Function(vec![Type::Any], Box::new(Type::Array(Box::new(Type::Any))))
            } else {
                let inner = (**elem).clone();
                Type::Function(
                    vec![Type::Array(Box::new(inner.clone()))],
                    Box::new(Type::Array(Box::new(inner))),
                )
            }
        }
        // `xs.at(i)` — element at i with negative-index wrap.
        // Subset returns T (not T | undefined) — out-of-bounds
        // is UB, matches the unchecked indexing convention.
        (Type::Array(elem), "at") => Type::Function(vec![Type::Number], Box::new((**elem).clone())),
        // `xs.reverse()` — in-place reverse, returns the same
        // array (chainable). Subset returns void since the
        // chain shape isn't common in our test set.
        // `toReversed` (ES2023) is the non-mutating sibling —
        // identical signature, fresh `Array<T>` result.
        (Type::Array(elem), "reverse" | "toReversed") => Type::Function(
            Vec::new(),
            Box::new(Type::Array(Box::new((**elem).clone()))),
        ),
        // `xs.with(i, v)` (ES2023) — non-mutating index update.
        // Returns a fresh `Array<T>` with `xs[i] = v`. Negative
        // `i` wraps via `len + i`. OOB is UB.
        (Type::Array(elem), "with") => {
            let inner = (**elem).clone();
            Type::Function(
                vec![Type::Number, inner.clone()],
                Box::new(Type::Array(Box::new(inner))),
            )
        }
        // `xs.copyWithin(target, start, end)` — memmove
        // [start, end) into `target` position, in-place.
        (Type::Array(elem), "copyWithin") => Type::Function(
            vec![Type::Number, Type::Number, Type::Number],
            Box::new(Type::Array(Box::new((**elem).clone()))),
        ),
        // `xs.fill(value, start, end)` — uniform fill over a
        // range. start/end optional in JS; subset requires
        // both for now. Returns the same array.
        (Type::Array(elem), "fill") => {
            let inner = (**elem).clone();
            Type::Function(
                vec![inner.clone(), Type::Number, Type::Number],
                Box::new(Type::Array(Box::new(inner))),
            )
        }
        // `xs.slice(start, end)` — fresh array of the
        // [start, end) range. Same element type. Both
        // bounds are required in this v0 subset.
        (Type::Array(elem), "slice") => Type::Function(
            vec![Type::Number, Type::Number],
            Box::new(Type::Array(Box::new((**elem).clone()))),
        ),
        // `xs.indexOf(needle)` / `xs.lastIndexOf(needle)` —
        // linear scan; returns -1 on miss. lastIndexOf scans
        // from the end. Needle must match the element type.
        (Type::Array(elem), "indexOf" | "lastIndexOf") => {
            Type::Function(vec![(**elem).clone()], Box::new(Type::Number))
        }
        _ => return None,
    };
    Some(ty)
}

/// Spec §23.1.3 callback shape — `(elem, index, sourceArray) => ret`.
/// Declaring the trailing slots lets full-arity user callbacks admit
/// (shorter ones ride the S133 prefix rule as before); the lowering
/// appends the actual index / source-array values per the callback's
/// own declared arity. The sourceArray slot is `Array<Any>` — the
/// kind-aware view (chunk 625/626 protocol) that reads i64-slot
/// numeric blocks correctly; it also matches the `any[]` annotation
/// the closure-param inference seeds.
fn cb3(elem: &Type, ret: Type) -> Type {
    Type::Function(
        vec![elem.clone(), Type::Number, Type::Array(Box::new(Type::Any))],
        Box::new(ret),
    )
}

/// Second half — callback-iteration arms (map → some/every), the
/// keys/values/entries ArrIter arms, and the T-29 Array-as-Object
/// catch-all (must stay the LAST arm so it only fires after every
/// typed-array dispatch above misses).
fn try_match_iter(obj_ty: &Type, name: &str) -> Option<Type> {
    let ty = match (obj_ty, name) {
        // M6.2 — `xs.map(fn)`: takes a `(T) => T` closure,
        // returns `T[]` (a fresh array). MVP keeps input
        // and output element types the same; non-uniform
        // map (e.g. `number[] → string[]`) lands when
        // generic methods are wired (post-M6.2.a).
        (Type::Array(elem), "map") => {
            let inner = (**elem).clone();
            Type::Function(
                vec![cb3(&inner, inner.clone())],
                Box::new(Type::Array(Box::new(inner))),
            )
        }
        // `xs.flatMap(fn)` — same homogeneous constraint as
        // map (`(T) => T[]` callback), returns `T[]`. Inner
        // arrays are flattened one level into the result. The
        // callback takes the full §23.1.3 spec arity (elem,
        // index, srcArray) — shorter callbacks ride the S133
        // prefix rule, and the lowering appends index/srcArray
        // per the callback's own declared arity (rotation 286).
        (Type::Array(elem), "flatMap") => {
            let inner = (**elem).clone();
            let arr_t = Type::Array(Box::new(inner.clone()));
            Type::Function(vec![cb3(&inner, arr_t.clone())], Box::new(arr_t))
        }
        // M6.2 — `xs.filter(predicate)`: takes a `(T) => boolean`,
        // returns `T[]` of kept elements.
        (Type::Array(elem), "filter") => {
            let inner = (**elem).clone();
            Type::Function(
                vec![cb3(&inner, Type::Boolean)],
                Box::new(Type::Array(Box::new(inner))),
            )
        }
        // M6.2 — `xs.reduce(fn, initial)`: takes a
        // `(acc: T, x: T) => T` and an initial T value;
        // returns T. Two-arg reduce; the no-initial overload
        // is deferred.
        // S132 — reduceRight: identical signature to reduce,
        // but walks last → first (spec §22.1.3.22). ssa-lower
        // shares the loop scaffold with `reduce`, differing
        // only in the cursor init / cmp / inc direction.
        (Type::Array(elem), "reduce" | "reduceRight") => {
            let inner = (**elem).clone();
            // Reducer shape per §23.1.3.24 — (acc, cur, index, srcArray);
            // srcArray is the kind-aware Array<Any> view (see cb3).
            Type::Function(
                vec![
                    Type::Function(
                        vec![
                            inner.clone(),
                            inner.clone(),
                            Type::Number,
                            Type::Array(Box::new(Type::Any)),
                        ],
                        Box::new(inner.clone()),
                    ),
                    inner.clone(),
                ],
                Box::new(inner),
            )
        }
        // M6.2 — `xs.forEach(fn)`: takes a `(T) => void`,
        // returns void. Used for side-effecting iteration.
        (Type::Array(elem), "forEach") => {
            Type::Function(vec![cb3(elem, Type::Void)], Box::new(Type::Void))
        }
        /* P6.4c-C3 / P5.4 — Array.keys / .values / .entries
         * returning ArrIter, for ANY element type. `.keys()`
         * yields 0..length-1 independent of the slot encoding;
         * `.values()` / `.entries()` route through the ArrIter
         * step's kind-aware `__torajs_arr_get_any_boxed`, which
         * reboxes a typed (8B-slot) Array<T> per its recorded
         * elem kind. The `.values()` / `.entries()` lowering
         * (`ssa_lower_call_arr_iter_ctor`) emits
         * `__torajs_arr_mark_kind` on the receiver first so the
         * kind is recorded (a typed literal is otherwise
         * ARR_KIND_UNSET → the rebox would answer undefined). */
        (Type::Array(_), "keys" | "values" | "entries") => {
            Type::Function(Vec::new(), Box::new(Type::ArrIter))
        }
        // `xs.includes(needle)` — boolean variant of indexOf.
        (Type::Array(elem), "includes") => {
            Type::Function(vec![(**elem).clone()], Box::new(Type::Boolean))
        }
        // `xs.find(p)` / `xs.findLast(p)` — predicate scan.
        // tr's subset returns the element type itchecker (no
        // `T | undefined`); not-found returns the zero of
        // T (null for refcounted, 0 / false for primitives).
        // Caller can either disambiguate via findIndex first
        // or check against the sentinel value.
        (Type::Array(elem), "find" | "findLast") => {
            let inner = (**elem).clone();
            Type::Function(vec![cb3(&inner, Type::Boolean)], Box::new(inner))
        }
        // `xs.findIndex(pred)` — index of first matching, or -1.
        // `findLastIndex` is the reverse-iteration sibling and
        // shares the same -1-on-miss return.
        (Type::Array(elem), "findIndex" | "findLastIndex") => {
            Type::Function(vec![cb3(elem, Type::Boolean)], Box::new(Type::Number))
        }
        // `xs.some(pred)` / `xs.every(pred)` — short-circuit
        // ored / anded predicate iteration.
        (Type::Array(elem), "some" | "every") => {
            Type::Function(vec![cb3(elem, Type::Boolean)], Box::new(Type::Boolean))
        }
        // T-29 — Array-as-Object catch-all read. `arr.x` on
        // an array with an unknown name returns Type::Any
        // (lookup via side table at lower time). Excludes
        // `length` (handled by chunk 205 prim-union arm) +
        // every built-in array method name (those guard-failing
        // here return `None` so the main match's `_ => Err`
        // arm fires with a per-method message — e.g.
        // `arr.join` on Array<Struct> typecheck-errors instead
        // of silently degrading to Any). Must remain the LAST
        // arm in this match so it only fires after every
        // typed-array dispatch above misses.
        (Type::Array(_), n) if n != "length" && !is_array_method_name(n) => Type::Any,
        _ => return None,
    };
    Some(ty)
}
