// `o.k++` ends in PutValue (§13.4.4.1 step 5), so it owes the §10.1.9
// writable gate exactly what `o.k = v` owes it. The struct-field lane
// emitted a bare Store at the field offset, so a frozen instance's
// `o.hit++` wrote THROUGH the freeze and answered "after" — while the
// plain-assign spelling of the same field on the same object threw.
class C {
  hit = 1;
}
const a = new C();
Object.freeze(a);
try {
  a.hit++;
  console.log("no throw, hit =", a.hit);
} catch (e) {
  console.log("caught", (e as Error).constructor.name, "hit =", a.hit);
}
// The plain-assign spelling, for contrast — this one has thrown since
// rotation 373.
const b = new C();
Object.freeze(b);
try {
  b.hit = 5;
  console.log("no throw, hit =", b.hit);
} catch (e) {
  console.log("caught", (e as Error).constructor.name, "hit =", b.hit);
}
// Decrement is the same lane.
const c = new C();
Object.freeze(c);
try {
  c.hit--;
  console.log("no throw, hit =", c.hit);
} catch (e) {
  console.log("caught", (e as Error).constructor.name, "hit =", c.hit);
}
// An UNFROZEN instance must still step normally (the guard's fast
// path is the whole point of it being a guard).
const d = new C();
d.hit++;
d.hit++;
console.log("unfrozen hit =", d.hit);
