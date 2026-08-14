// 402-01 face 1 — a top-level generic fn escaping into the any
// world: the `__forward_` shim erases the target's TypeVars to
// `any`, and the mono pass instantiates the target at Any for the
// shim body's call.
function gid<T>(v: T): T { return v }
function pair<A, B>(a: A, b: B): A[] { return [a, a] }
const f: any = gid;
console.log(f(9));
console.log(f("s"));
const p: any = pair;
console.log(p(3, "x"));
const g = gid;
console.log(g(7));
let h: any;
h = gid;
console.log(h(5));
