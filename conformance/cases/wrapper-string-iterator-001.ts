// RFC 20260716-primitive-wrapper-substrate 刀 12 — StringWrapper
// receiver in the `for..of` / indexed-iteration lane. Pre-fix
// `for (const c of new String("abc"))` threw "value is not
// iterable" because `__torajs_any_iter_next`'s `indexed` gate
// only recognized `Tag::Str` / `Tag::Arr` / short-str, and
// `__torajs_any_iter_len` refused the wrapper tag.
//
// Fix path: the wrapper cell already view-throughs on
// `__torajs_any_index_get` (刀 3) — only the two iteration hooks
// needed the arm. `iter_len` reads the inner cell's Str length
// (with the `MIRROR_FLAG_SUBSTR_INLINE` branch); `iter_next`
// treats `Tag::StringWrapper` as indexed so the per-step
// `recv[i]` reads the wrapper the same way `str[i]` does.
// NumberWrapper / BooleanWrapper stay non-iterable (bun throws
// the same TypeError there).

for (const c of new String("hi")) console.log(c);
// h
// i

// Empty wrapper — length 0 → zero iterations, no throw.
for (const c of new String("")) console.log("wont-fire");
console.log("empty-done");

// Bare wrapper on the iteration RHS (no destructure), fold via
// counter — proves indexed iteration is stateless per step.
let seen = 0;
for (const _c of new String("abcd")) seen++;
console.log(seen);
// 4

// Non-iterable wrapper — Number / Boolean wrappers still throw
// per spec (Object.prototype has no `[Symbol.iterator]`).
try {
    for (const x of new Number(42) as any) console.log(x);
    console.log("Number wrapper should have thrown");
} catch (e) {
    console.log("Number wrapper: TypeError");
}
try {
    for (const x of new Boolean(true) as any) console.log(x);
    console.log("Boolean wrapper should have thrown");
} catch (e) {
    console.log("Boolean wrapper: TypeError");
}
