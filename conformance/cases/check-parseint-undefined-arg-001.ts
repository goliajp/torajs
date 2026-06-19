// S226 — parseInt / parseFloat / Number.parseInt / Number.parseFloat
// with an explicit `undefined` arg per ES §19.2.5.1 / §19.2.4.1 /
// §21.1.2.13 / §21.1.2.12. Step 1 reads `string` which is then
// passed through ToString. `ToString(undefined) = "undefined"` —
// not a valid number prefix — so the parse fails and the result
// is NaN. Same fold the 0-arg form already takes (S202).

console.log(parseInt(undefined))
console.log(parseInt(undefined, 16))
console.log(parseFloat(undefined))
console.log(Number.parseInt(undefined))
console.log(Number.parseInt(undefined, 10))
console.log(Number.parseFloat(undefined))

// non-undef regression — string + numeric radix still work.
console.log(parseInt("42"))
console.log(parseInt("ff", 16))
console.log(parseFloat("3.14"))
console.log(Number.parseInt("100", 2))
console.log(Number.parseFloat("2.71"))

// 0-arg regression — S202 fold.
console.log(parseInt())
console.log(parseFloat())
console.log(Number.parseInt())
console.log(Number.parseFloat())
