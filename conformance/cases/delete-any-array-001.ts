// §13.5.1.2 over an `any`-element array binding. Deleting is not
// writing a blank: it removes an own property, so the slot has to be
// able to say "no longer here". A boxed element slot can, which is why
// this receiver reaches the OrdinaryDelete kernel while an unboxed one
// (`number[]`) does not.

const a: any[] = ["x", "y", "z"];
console.log(delete a[1]);
console.log(1 in a, a[1], a.length);
console.log(JSON.stringify(a));
console.log(Object.keys(a).join(","));

// refcounted elements release through the same route
const c: any[] = [{ p: 1 }, { p: 2 }];
console.log(delete c[0], JSON.stringify(c));

// §10.4.2 — an array's `length` is permanently non-configurable, and
// module code is strict, so the refusal throws.
const b: any[] = [1, 2];
try {
  delete b.length;
  console.log("no throw");
} catch (e) {
  console.log("threw");
}
console.log(b.length);

// an absent index is a spec success
console.log(delete b[99]);

// iteration sees the hole
const d: any[] = [1, 2, 3];
console.log(delete d[1]);
let sum = 0;
for (const v of d) {
  sum += v;
}
console.log(sum);
console.log(d.indexOf(3));
