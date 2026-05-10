// V3-18 m1.h.52 — coercion functions on `undefined`:
//   Number(undefined)  → NaN     (per JS spec §7.1.4 ToNumber)
//   String(undefined)  → "undefined"  (§7.1.17 ToString)
//   Boolean(undefined) → false   (§7.1.2 ToBoolean)
//
// Pre-fix tora's Number/String/Boolean callable arms saw the bare
// `undefined` as Type::Ptr (collapsed with null at the runtime
// layer) so Number(undefined) returned 0 instead of NaN.
//
// Implementation: detect `Expr::Ident("undefined")` at the
// call-site BEFORE arg lowering, since the runtime layer has no
// distinct Undefined sentinel.

console.log(Number(undefined))      // NaN
console.log(Number(null))           // 0 (no regression)
console.log(Number(""))             // 0
console.log(Number("3"))            // 3
console.log(Number(true))           // 1
console.log(Number(false))          // 0

console.log(String(undefined))      // undefined
console.log(String(null))           // null
console.log(String(true))           // true
console.log(String(false))          // false
console.log(String(42))             // 42

console.log(Boolean(undefined))     // false
console.log(Boolean(null))          // false
console.log(Boolean(0))             // false
console.log(Boolean(1))             // true
console.log(Boolean(""))            // false
console.log(Boolean("a"))           // true
