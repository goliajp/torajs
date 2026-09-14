// A GENERIC `this`-reading declaration at a thisArg callback position
// mints a receiver-first shell (`__fwdrecv_g`) from the target's
// signature — which still spells the target's own type variables. The
// erasure to `any` (402-01) lived in the member-assigned mint alone,
// so this shell died at the checker ("unknown type `T` for parameter
// `a` of function `__fwdrecv_g`"). The one synthesizer erases for
// every face now (rotation 650).
const o = { k: 2 };
function g<T>(this: any, a: T): any { return [arguments.length, a, (this as any).k]; }
console.log(JSON.stringify([3].map(g, o)));
console.log(JSON.stringify(["s"].map(g, o)));
const xs: any = [4];
console.log(JSON.stringify(xs.map(g, o)), JSON.stringify(xs.filter(g, o)));
function h<T>(this: any, a: T, b: T): any { return [a, b, typeof this]; }
console.log(JSON.stringify((h as any)(1, 2)));
// `.call` / `.apply` on the generic declaration (rotation 651): the
// member-base axis wrapped `h` into the plain shell before the
// fn-proto desugar could see it, so the thisArg was pinned to
// `undefined` — the non-generic twin below was right all along.
console.log(JSON.stringify(h.call(o, "x", "y")), JSON.stringify(h.apply(o, ["x", "y"])));
console.log(JSON.stringify(g.call(o, 5)), JSON.stringify(g.apply(o, [6])));
function h2(this: any, a: any, b: any): any { return [a, b, typeof this]; }
console.log(JSON.stringify(h2.call(o, "x", "y")));
function id<T>(x: T): T { return x; }
console.log(id.call(null, 7), id.apply(null, ["q"]));
