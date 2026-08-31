// a `let` / `const` is a statement, so the sentinel pre-scan that
// walks the expression arena could not see what its init put in the
// binding; everything downstream of one answered NaN.
const zs: number[] = [1, 2, 3];

const u = zs[9];
const r = { v: u };
console.log(r.v, typeof r.v, r.v === undefined);

const s = { w: 0 };
s.w = u;
console.log(s.w, typeof s.w);

let a: number = 0;
a = u;
console.log(a, a === undefined);

// nested bodies: a fn body, a block, and a loop body each hold the
// declaration the walk has to reach.
function inFn(): void {
  const q = zs[9];
  const t = { z: q };
  console.log(q, t.z, typeof t.z);
}
inFn();

{
  const b = zs[9];
  const c = { y: b };
  console.log(c.y);
}

for (let i: number = 0; i < 1; i++) {
  const d = zs[9];
  const e = { x: d };
  console.log(e.x);
}

// two hops — the fixpoint has to run more than once for `m` to be
// known before `n` is asked about.
const m = zs[9];
const n = m;
const o = { p: n };
console.log(o.p, typeof o.p);

// an in-range read stays a number through the same chain.
const good = zs[1];
const g2 = { v: good };
console.log(g2.v, typeof g2.v, g2.v === undefined);
