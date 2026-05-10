// V3-18 m1.h.24 — Math.max/min with 0 or 1 args. Per JS spec
// §21.3.2.24 / §21.3.2.25:
//   Math.max() = -Infinity (identity element)
//   Math.min() = +Infinity (identity element)
//   Math.max(x) = x  (or +0 vs -0 etc, here we just check value)
// Pre-fix tora hard-rejected with "Math.X requires at least 2
// args, got N".

console.log(Math.max())
console.log(Math.min())
console.log(Math.max(5))
console.log(Math.min(5))
console.log(Math.max(1, 2, 3))
console.log(Math.min(5, 3, 1))
console.log(Math.max(1, 2, 3, 4, 5, 6, 7))
console.log(Math.min(7, 6, 5, 4, 3, 2, 1))
