// chunk 784 — the param's declared struct layout pins a direct
// ObjectLit call arg (declared-layout hint at the call-arg site):
// without it the same-shaped literal first-matched B's I64 layout
// while A declares an Any (nullable) slot — ta({ v: 3 }) silently
// read 0.
type A = { v?: number };
type B = { v: number };
function ta(a: A): number { return a.v ?? 0 }
function tb(b: B): number { return b.v }
console.log(tb({ v: 2 }));
console.log(ta({ v: 3 }));
