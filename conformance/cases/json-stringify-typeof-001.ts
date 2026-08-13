// §25.5.2 — `JSON.stringify` answers undefined for a value that has no
// JSON representation. The VALUE was already right; only `typeof` was
// wrong, because it folded from the static type.
//
// The static type stays `String` on purpose: that is what TS's
// lib.d.ts declares, so `JSON.stringify(x).length` has to keep
// compiling — bun runs it. Typing the result "string or undefined"
// instead makes the checker refuse `.length`, `String(...)` and `+`
// on it, which is a step away from running the programs bun runs. So
// the undefined-ness rides the nullable-source guard, where the
// consumers that cannot be answered statically take a runtime branch.
const nothing: any = undefined;
console.log(JSON.stringify(nothing), typeof JSON.stringify(nothing));
console.log(JSON.stringify(nothing) === undefined);

const fn: any = function () {};
console.log(JSON.stringify(fn), typeof JSON.stringify(fn));

const sym: any = Symbol("s");
console.log(JSON.stringify(sym), typeof JSON.stringify(sym));

// Everything with a representation keeps answering "string".
console.log(JSON.stringify({ a: 1 }), typeof JSON.stringify({ a: 1 }));
console.log(JSON.stringify([1, 2]), typeof JSON.stringify([1, 2]));
console.log(JSON.stringify("s"), typeof JSON.stringify("s"));
console.log(JSON.stringify(42), typeof JSON.stringify(42));
console.log(JSON.stringify(null), typeof JSON.stringify(null));

// The string face still works without a cast — the point of keeping
// the static type `String`.
console.log(JSON.stringify({ a: 1 }).length);
console.log("[" + JSON.stringify(7) + "]");
