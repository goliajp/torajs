// chunk 710 — Function.prototype.call / apply on any-world closures.
const h: any = (x: number, y: number) => x + y;
console.log(h.call(null, 5, 6));
console.log(h.apply(null, [7, 8]));
console.log(h(2, 3));

const one: any = (s: string) => s + "!";
console.log(one.call("this-ignored", "hi"));
console.log(one.apply(undefined, ["yo"]));

const cat: any = (a: string, b: string) => a + b;
console.log(cat.apply(null, ["x", "y"]));

// empty / missing list shapes
const z: any = () => 42;
console.log(z.call(null));
console.log(z.apply(null));
console.log(z.apply(null, []));

// closure read out of a dynobj field, then .call on the value
const o: any = { m: (a: number) => a * 3 };
const g = o.m;
console.log(g.call(null, 7));
console.log(o.m(2));

// apply with a non-array list is a TypeError
try {
  h.apply(null, 123);
} catch (e) {
  console.log("apply-non-array threw:", e instanceof TypeError);
}

// an expando shadows the builtin per ES own-property order
const f2: any = (n: number) => n * 2;
f2.call = (n: number) => n + 100;
console.log(f2.call(1));
