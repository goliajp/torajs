// Symbol.keyFor answers string | undefined (ES §20.4.2.6) — the
// unregistered face is undefined, never null, on every arg shape
const u = Symbol("un");
const r = Symbol.for("reg3");
const a: any = Symbol.for("reg3");
console.log(Symbol.keyFor(u), Symbol.keyFor(r), Symbol.keyFor(a));

const k = Symbol.keyFor(u);
console.log(k, typeof k, typeof Symbol.keyFor(r));

// reading keyFor as a value keeps the same answer shape
const f = Symbol.keyFor;
console.log(f(r));
console.log(f(u));

// registry identity holds across the route
console.log(Symbol.for("rt") === Symbol.for("rt"));
