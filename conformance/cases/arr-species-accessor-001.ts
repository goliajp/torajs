// ArraySpeciesCreate reads @@species with a Get, so an accessor-shaped
// entry has to run its getter. The symbol probe answers an ACCESSOR
// sentinel; it is neither undefined nor null, so it slipped past both
// default checks and got boxed as if it were the species itself — the
// "array species constructor is not a constructor" TypeError was that
// sentinel arriving at Construct.

class SubA extends Array {}
Object.defineProperty(SubA, Symbol.species, {
  get() { return Array; },
});
const a: any = new SubA();
a.push(1, 2);
console.log(a.map((x: any) => x).constructor === Array); // true
console.log(a.filter((x: any) => x > 1).constructor === Array); // true

// a species getter that throws propagates instead of constructing
class SubB extends Array {}
Object.defineProperty(SubB, Symbol.species, {
  get() { throw new Error("species"); },
});
const b: any = new SubB();
b.push(1);
try {
  b.map((x: any) => x);
} catch (e: any) {
  console.log("caught", e.message);
} // caught species

// data-property species is unchanged
class SubC extends Array {}
Object.defineProperty(SubC, Symbol.species, { value: Array });
const c: any = new SubC();
c.push(1);
console.log(c.map((x: any) => x).constructor === Array); // true

// no @@species at all still derives into the subclass
class SubD extends Array {}
const d: any = new SubD();
d.push(1);
console.log(d.map((x: any) => x).constructor === SubD); // true

// plain arrays are untouched
const p = [1, 2, 3];
console.log(p.map((x) => x * 2).join(","), p.slice(1).join(",")); // 2,4,6 2,3
