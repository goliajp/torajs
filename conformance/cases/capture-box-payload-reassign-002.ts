// The same shape with the per-turn `let` read from the closure, a
// nested loop, and enough turns to cross the cycle collector's
// automatic threshold many times.
let g = { t: 0, u: "a" };
let sum = 0;
for (let i = 0; i < 100000; i++) {
  const f = () => { g = { t: i, u: "x" + (i % 7) }; };
  f();
  sum += g.t;
}
console.log(g.t, g.u, sum);

let h = { k: 0 };
for (let i = 0; i < 4; i++) {
  for (let j = 0; j < 5; j++) {
    const f = () => { h = { k: i * 10 + j }; };
    f();
  }
}
console.log(h.k);
