// §10.4.2 [[Delete]] REMOVES a property; it never writes to one. The
// array delete cleared the element slot through the ordinary index
// store first, which on an accessor index runs the setter — and a
// getter-only pair answers the readonly TypeError, so
// `delete arr[1]` threw instead of deleting.
const a: any[] = [1, 2, 3];
Object.defineProperty(a, "1", { get() { return 99; }, configurable: true });
console.log(a[1], 1 in a);
console.log(delete a[1]);
console.log(a[1], 1 in a, a.length);

// A get/set pair: the setter must NOT run either.
const b: any[] = [1, 2, 3];
let stored = 0;
Object.defineProperty(b, "1", {
  get() { return 7; },
  set(v: any) { stored = v; },
  configurable: true,
});
console.log(delete b[1], stored, 1 in b);

// A non-configurable accessor still refuses the delete (§10.4.2.4
// step 5 answers false, and §13.5.1.2 step 5.b makes that a TypeError
// in strict code — module code always is).
const c: any[] = [1, 2, 3];
Object.defineProperty(c, "1", { get() { return 5; }, configurable: false });
try { delete c[1]; console.log("NO THROW"); }
catch (e: any) { console.log(e.constructor.name); }
console.log(1 in c, c[1]);

// The plain-element path is unchanged.
const d: any[] = [1, 2, 3];
console.log(delete d[1], d[1], 1 in d, d.length);
console.log(JSON.stringify(a), JSON.stringify(d));
