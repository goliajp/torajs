// P9.4 follow-up — `RegExp.prototype.test()` honors sticky / global
// lastIndex per ES spec §22.2.5.2.2 (test ≡ exec !== null). Pre-fix
// tora always searched from index 0 ignoring lastIndex, silently
// disagreeing with bun on every sticky / global regex .test() walk.

// Plain (no g, no y) — lastIndex ignored, never written
const plain = /ab/
plain.lastIndex = 5
console.log(plain.test('xabxab'))      // true (search from 0)
console.log(plain.lastIndex)           // 5 (unchanged)

// Global (g) — starts at lastIndex, advances on hit, resets on miss
const g = /ab/g
console.log(g.lastIndex)               // 0
console.log(g.test('xabxab'))          // true (hits at 1)
console.log(g.lastIndex)               // 3 (end of first match)
console.log(g.test('xabxab'))          // true (hits at 4 from start=3)
console.log(g.lastIndex)               // 6 (end of second match)
console.log(g.test('xabxab'))          // false (start=6, slen=6, miss)
console.log(g.lastIndex)               // 0 (reset on miss)

// Sticky (y) — anchored at lastIndex, single attempt
const y = /ab/y
console.log(y.lastIndex)               // 0
console.log(y.test('xabxab'))          // false (no anchor at 0)
console.log(y.lastIndex)               // 0 (reset on miss)
y.lastIndex = 1
console.log(y.test('xabxab'))          // true (anchored at 1)
console.log(y.lastIndex)               // 3
console.log(y.test('xabxab'))          // false (anchored at 3 = 'x', miss)
console.log(y.lastIndex)               // 0 (reset on miss)

// y + g together — y wins per spec
const yg = /ab/gy
yg.lastIndex = 1
console.log(yg.test('xabxab'))         // true (sticky anchored at 1)
console.log(yg.lastIndex)              // 3

// lastIndex past string length on tracking regex → immediate miss
const past = /ab/g
past.lastIndex = 999
console.log(past.test('xabxab'))       // false
console.log(past.lastIndex)            // 0 (reset)
