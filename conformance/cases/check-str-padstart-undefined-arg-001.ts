// S223 — String.{padStart,padEnd}(undefined [, fillStr]) per ES
// §22.1.3.{16,17} step 1: ToLength(undefined) = 0, then step 2
// short-circuits since 0 <= S.length, so the call returns S
// unchanged. Cover both the 1-arg and 2-arg explicit-undefined
// shapes for both methods.

const s = "abc"

console.log(JSON.stringify(s.padStart(undefined)))
console.log(JSON.stringify(s.padStart(undefined, "*")))
console.log(JSON.stringify(s.padEnd(undefined)))
console.log(JSON.stringify(s.padEnd(undefined, "*")))

// non-undef cases should still work (regression check).
console.log(JSON.stringify(s.padStart(6)))
console.log(JSON.stringify(s.padStart(6, "*")))
console.log(JSON.stringify(s.padEnd(6)))
console.log(JSON.stringify(s.padEnd(6, "*")))

// empty receiver — same short-circuit applies.
console.log(JSON.stringify("".padStart(undefined)))
console.log(JSON.stringify("".padEnd(undefined, "*")))
