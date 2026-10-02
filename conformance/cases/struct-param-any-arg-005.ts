// §10.2.1.3 — the argument a struct-typed parameter receives through
// `any` is the value itself: `return p` answers the same array, Proxy
// or object. A declared `: number` return does not turn the undefined
// an array answers for `x` into a number.
type P = { x: number; y: number };
function id(p: P) { return p; }
function fx(p: P): number { return p.x; }
function f(p: P): number { const q = p; return q.x; }
const gid: any = id;
const gfx: any = fx;
const g: any = f;
const arr: any = [1, 2];
const px: any = new Proxy({}, {});
console.log("arr identity ->", gid(arr) === arr);
console.log("proxy identity ->", gid(px) === px);
const o = { x: 1, y: 2 };
console.log("obj identity ->", gid(o) === o);
console.log("ret number arr ->", gfx(arr));
console.log("ret number obj ->", gfx({ x: 9, y: 1 }));
console.log(g([1, 2, 3]));
console.log(g("abc"));
console.log(g(new Proxy({}, {})));
console.log("typed", fx({ x: 1, y: 2 }), f({ x: 3, y: 4 }));
