// r506 — float_demote's post-op growth guard on an accumulator whose
// add is the LAST instruction of its block. The versioning split
// asked for a continuation at `insts.len()` and got none, so the
// guard was never installed while the loop still shipped as the
// unguarded i64 version: past 2^53 the i64 sum stayed exact where
// bun's f64 sum rounds (a), or wrapped outright (b). Every step is a
// loop-internal value with an interval fact — that is what routes the
// accumulator through float_demote; a literal step never gets here.
let total = 0;
for (let i = 0; i < 150000000; i++) total += i;
console.log(total);
// cubes: the i64 sum wraps, the f64 sum rounds
let g = 0;
for (let i = 1; i < 100000; i++) g += i * i * i;
console.log(g);
// the step computed into a binding one instruction earlier
let h = 0;
for (let i = 1; i < 3000000; i++) {
  const c = i * i;
  h += c;
}
console.log(h);
// the small accumulator (never near 2^53) keeps its exact answer
let u = 0;
for (let i = 0; i < 300; i++) u += i;
console.log(u);
