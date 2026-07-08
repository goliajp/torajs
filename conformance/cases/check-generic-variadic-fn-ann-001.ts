// generic fn with a rest-tail fn-type param ann (chunk 682):
// `g<T>(cb: (...args: T[]) => T, x: T)` — the closure-hint pass
// resolves T at the call site before projecting spellings, and
// unify_typevar matches a fixed-arity closure against the rest tail.
function g<T>(cb: (...args: T[]) => T, x: T): T { return cb(x); }
console.log(g((a: number) => a * 2, 21));
// multi-param closure against the rest tail
function h<T>(cb: (...args: T[]) => T, x: T, y: T): T { return cb(x, y); }
console.log(h((a: number, b: number) => a + b, 40, 2));
// string lane
console.log(g((s: string) => s + "!", "ok"));
// typevar pinned only through the closure's own inferred ret
function q<T>(cb: (...args: T[]) => T): T { return cb(); }
console.log(q(() => 1));
// non-generic hint regression (chunk 554 face 2 shape)
function apply(f: (n: number) => number, v: number): number { return f(v); }
console.log(apply((n) => n + 1, 41));
// plain generic regression (mono track)
function id<T>(v: T): T { return v; }
console.log(id(7));
