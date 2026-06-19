// S230 — `Date.parse(undefined)` per ES §21.4.3.2 step 1: the arg
// is read through ToString. `ToString(undefined) = "undefined"`
// which is not a valid date string, so the result is NaN.

console.log(Date.parse(undefined))

// non-undef regression — valid ISO string still parses.
console.log(Date.parse("2024-01-01"))
console.log(Date.parse("1970-01-01T00:00:00Z"))

// invalid-string regression — NaN.
console.log(Date.parse("not-a-date"))
