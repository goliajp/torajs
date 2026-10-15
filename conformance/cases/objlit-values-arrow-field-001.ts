// Object.values / Object.entries over a literal whose function fields
// never read their own `this` (an arrow, a this-free method): the
// fields read out are ordinary functions and the literal reads back
// as written.
const h = { n: 1, cb: () => 2 };
const hv = Object.values(h);
console.log(hv.length, (hv[1] as any)(), h.cb(), h.n);

const k = { n: 3, twice(x: number) { return x * 2 }, inc: (x: number) => x + 1 };
const ke = Object.entries(k);
console.log(ke.map((e) => e[0]).join(","), (ke[1][1] as any)(4), k.twice(5), k.inc(k.n));
k.n = 9;
console.log(k.n, k.inc(1));
