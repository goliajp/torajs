// ES §22.1.3.6 - Number.prototype.toPrecision called with no argument
// returns ToString(this Number value) (precision is undefined, not 0).
// Pre-fix tr padded missing arg to `0` and called the standard
// toPrecision helper with `precision = 0`, which is outside the
// spec-defined `1 <= precision <= 100` range and produced bogus
// trailing zeros (e.g. "1234.50" instead of "1234.5").

console.log((1234.5).toPrecision())
console.log((0.000123).toPrecision())
console.log((1).toPrecision())
console.log((0).toPrecision())
console.log((42).toPrecision())
console.log((3.14).toPrecision())

// 1-arg path is unchanged (regression check).
console.log((1234.5).toPrecision(3))
console.log((1234.5).toPrecision(6))
