// V3-18 m1.h.12 — `console.log(arr)` pretty-print matching bun:
//   []          for empty (deferred — empty literal still needs
//               type annotation; tested via the runtime helper's
//               len==0 fast path elsewhere)
//   [ a, b, c ] for non-empty (note spaces)
// One runtime helper per element type (I64 / F64 / Bool / Str).
// Foundational — every test262 case that prints an array would
// otherwise show raw pointer values, diverging from bun on the
// first byte.
console.log([1, 2, 3])
console.log(["a", "b"])
console.log([true, false])
console.log([1.5, 2.5, 3.5])
let nums: number[] = [10, 20, 30, 40]
console.log(nums)
