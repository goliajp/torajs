// Member-store face, Ident-RHS form (knife-2 use-shape channel):
// `o.m = f` / `o["k"] = f` count as face positions, so a const
// fn-expr binding whose remaining uses are all face reads / direct
// calls promotes to the receiver-first shape. Alias inits and other
// use shapes keep the loud reject (zero-alias bar).
const f = function () {
  return this.k * 3;
};
const o: any = {};
o.m = f;
o.k = 7;
console.log(o.m());

const h = function () {
  return this.n + 1;
};
const p: any = {};
p["m2"] = h;
p.n = 41;
console.log(p["m2"](), p.m2());

const tf = function () {
  return 5;
};
const q: any = {};
q.m = tf;
console.log(q.m(), tf());
