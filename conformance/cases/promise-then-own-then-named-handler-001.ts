// §27.2.5.4 is the builtin's body; `p.then(f)` is Invoke(p, "then")
// whatever `f` is spelled as. A top-level named fn as the handler
// used to skip the own-`then` split: the split boxes the handler
// through `__forward_<f>` in its patched arm, and a program whose
// only fn value was that handler minted no forwarder at all, so the
// call took the builtin lane and the override never ran.
function add1(v: number): number { return v + 1; }
let p: Promise<number> = Promise.resolve(41);
(p as any).then = function (f: any) { console.log("override"); return Promise.resolve(f(100)); };
let q = p.then(add1);
console.log(await q);
let r: Promise<number> = Promise.resolve(1);
(r as any).then = function (f: any) { console.log("override2"); return Promise.resolve(f(200)); };
console.log(await r.then((x: number) => x + 5));
// an unpatched typed cell keeps the builtin lane with a named handler
let s: Promise<number> = Promise.resolve(6);
console.log(await s.then(add1));
