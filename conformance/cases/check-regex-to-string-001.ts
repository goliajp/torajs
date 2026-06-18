// ES §22.2.6.13 — `RegExp.prototype.toString()` returns
// `/` + source + `/` + flags. Single runtime helper builds the
// composed string in one alloc; no SSA-level intermediate Str
// concat needed.
console.log((/foo/).toString());           // "/foo/"
console.log((/bar/gi).toString());         // "/bar/gi"
console.log((/(?:)/).toString());          // "/(?:)/"
console.log((/a\/b/gm).toString());        // "/a\\/b/gm"

// dynamic-arg constructor combined with toString (smoke for the
// `new RegExp(...)` ↔ `.toString()` round-trip)
const dyn1 = new RegExp("hello", "iy");
console.log(dyn1.toString());              // "/hello/iy"

// Spec-ordered flags: g, i, m, s, u, y (regardless of input order)
console.log((/x/ymig).toString());         // "/x/gimy"

// length of stringified output (basic byte-count sanity)
const re = /abc/g;
console.log(re.toString().length);         // 6
