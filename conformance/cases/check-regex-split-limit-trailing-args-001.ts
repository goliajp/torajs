// S326 — `s.split(re, limit, ...trailing)` per ES §22.1.3.21.
// Pre-S326 bugs in ssa_lower regex-receiver split arm:
//   (1) limit silently ignored — `s.split(/,/, 3)` returned full split
//   (2) trailing args silent-dropped at lower-time (step() didn't fire)
// Mirror of S282 (string-receiver path).

let s = 0;
const step = (v: number): number => {
  s++;
  return v;
};

// (1) limit clamps the result (limit < total parts)
const a = "a,b,c,d,e".split(/,/, step(3));
console.log("a.len=", a.length);
console.log("a=", a.join("|"));

// (2) limit > parts → no clamp
const b = "x,y".split(/,/, step(10));
console.log("b.len=", b.length);

// (3) limit=0 → empty
const c = "a,b,c".split(/,/, step(0));
console.log("c.len=", c.length);

// (4) trailing args eval-then-drop (step()-counter detection)
const d = "p,q,r".split(/,/, step(2), step(999), step(-1));
console.log("d.len=", d.length);
console.log("d=", d.join("|"));

// (5) global flag `/g` regex still clamped
const e = "1-2-3-4".split(/-/g, step(2));
console.log("e.len=", e.length);
console.log("e=", e.join("|"));

console.log("steps=", s);
