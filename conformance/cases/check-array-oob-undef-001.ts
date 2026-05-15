// P1.4 — Array<Any> OOB read returns undefined per ES spec
// §10.4.2.1 (sparse arrays return undefined for missing indices).
// Pre-P1.4 tora inlined the LoadDyn at `24 + (head + i) * 16`
// unconditionally without a bounds check; OOB returned garbage
// (often ANY_NULL by zero-padding luck), getting collapsed with
// null. Now OOB returns ANY_UNDEF=5 / value=0; the typeof /
// strict-eq paths route through the P1.5/P1.8 ANY_UNDEF
// behavior so the spec distinction is preserved end-to-end.
//
// Implementation:
// * runtime_str.c — `__torajs_arr_get_any_tag` / `_value` add
//   explicit `if (i >= len) return ANY_UNDEF;` checks.
// * ssa_lower.rs — Array<Any> Index read routes through the
//   bounds-checking helpers instead of inline LoadDyn. Trades
//   per-read function-call overhead for correctness; the helper
//   bodies are tiny and inline well at the LLVM level.
//
// Out of scope at this commit: `.at()` on Array<Any> has a pre-
// existing crash unrelated to the OOB-undef substrate; the
// fixture exercises only the `xs[i]` form.

let xs: any[] = [1, 2, 3]

// In-bounds reads stay correct.
console.log(xs[0])                            // 1
console.log(xs[1])                            // 2
console.log(xs[2])                            // 3

// OOB reads return undefined.
console.log(xs[99])                           // undefined
console.log(xs[3])                            // undefined (just past end)
console.log(xs[100000])                       // undefined

// typeof on OOB read is "undefined".
console.log(typeof xs[99])                    // undefined

// Strict equality distinguishes undefined from null.
console.log(xs[99] === undefined)             // true
console.log(xs[99] === null)                  // false

// Empty Array<Any> — every read OOB.
let empty: any[] = []
console.log(empty[0])                         // undefined
console.log(typeof empty[0])                  // undefined
console.log(empty[0] === undefined)           // true

// Heterogeneous + OOB.
let mixed: any[] = [1, "two", true]
console.log(mixed[0])                         // 1
console.log(mixed[1])                         // two
console.log(mixed[2])                         // true
console.log(mixed[3])                         // undefined
console.log(typeof mixed[3])                  // undefined
