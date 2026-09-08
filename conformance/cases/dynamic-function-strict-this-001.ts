// §20.2.1.1 step 17 + §10.2.1.2 step 3 — a dynamic function whose body
// opens with a 'use strict' prologue takes its `this` straight from the
// call, with no substitution and no ToObject. tr's substrate already
// binds `this` that way for every function it compiles, so the strict
// body needs no rewrite; what used to stand here was a reject.
const f = Function("'use strict'; return typeof this;");
console.log(f());

const g = Function("'use strict'; return this;");
const o = { tag: "o" };
console.log(g.call(o) === o);
console.log(g.call(7));
console.log(typeof g.call("s"));

// the shape test262's 13.2-12-s / 13.2-20-s use: enumerate the receiver
// from inside a strict dynamic function.
const h = Function("'use strict'; var n = 0; for (var k in this) { n = n + 1; } return n;");
console.log(h.call({ a: 1, b: 2 }));
console.log(h.call({}));

// a sloppy body still routes through the globalThis rewrite, unchanged.
const s = Function("return typeof this;");
console.log(s());
