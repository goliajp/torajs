// A function-valued field read out through Object.values /
// Object.entries is an ordinary function: `.call` hands it the given
// receiver, `new` a fresh object, and a direct call through the literal
// still reads the literal.
const p = { k: 1, f: function () { return this.k } };
const vs = Object.values(p);
console.log((vs[1] as any).call({ k: 6 }), p.f());

const q = { k: 1, F: function () { this.k = 3 } };
const es = Object.entries(q);
const made = new (es[1][1] as any)();
console.log(made.k, q.k, es[1][0]);
q.F();
console.log(q.k);
