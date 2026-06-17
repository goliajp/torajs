// ES §22.1.3.5 — `Number.prototype.toExponential()` (no arg) returns
// the shortest representation per Number::toString — NOT
// toExponential(0) which would emit a single-sig-digit mantissa.
// Pre-fix tr padded missing arg to 0 in ssa_lower, so:
//   `(1234567890).toExponential()` returned "1e+9"
// while bun (and the spec) gives "1.23456789e+9". Fix passes a -1
// sentinel for the no-arg form; the runtime helper routes that
// through Rust's ryu-shortest `{:e}` formatter.

// No-arg form — shortest
console.log((1234567890).toExponential())     // "1.23456789e+9"
console.log((1234.5678).toExponential())      // "1.2345678e+3"
console.log((0.1).toExponential())            // "1e-1"
console.log((1.5).toExponential())            // "1.5e+0"
console.log((1e-7).toExponential())           // "1e-7"
console.log((1e21).toExponential())           // "1e+21"
console.log((123).toExponential())            // "1.23e+2"

// Sign / zero edges
console.log((0).toExponential())              // "0e+0"
console.log((-0).toExponential())             // "0e+0"  (spec: -0 normalises)
console.log((-1234.5).toExponential())        // "-1.2345e+3"
// `(-0).toExponential(2)` also normalises the sign per spec
console.log((-0).toExponential(2))            // "0.00e+0"

// Explicit digits unchanged regression guards
console.log((1234.5678).toExponential(0))     // "1e+3"
console.log((1234.5678).toExponential(2))     // "1.23e+3"
console.log((1234.5678).toExponential(4))     // "1.2346e+3"
