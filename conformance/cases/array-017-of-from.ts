// v0.2 #7: Array.of (variadic) — rewritten to array literal at desugar.

const a = Array.of(1, 2, 3);
console.log(a.length, a[0], a[1], a[2]);

const b = Array.of("x", "y", "z");
console.log(b.length, b[0], b[2]);

const c = Array.of(42);
console.log(c.length, c[0]);

// Array.from over a string
const d = Array.from("abc");
console.log(d.length, d[0], d[1], d[2]);
