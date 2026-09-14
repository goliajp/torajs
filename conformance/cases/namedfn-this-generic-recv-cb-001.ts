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
