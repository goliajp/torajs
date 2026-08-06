// §17 and §21.3.1 — a built-in namespace's own properties are not
// ordinary writes. Every function property is non-enumerable, and the
// eight Math constants are non-writable, non-enumerable and
// non-configurable. Filling the namespace object with plain writes gave
// every entry a write's defaults, so the object answered a different
// fact to each surface that asked it.

const m: any = Math;
console.log(Object.keys(m).length);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(m, "PI")));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(m, "abs")));
console.log(JSON.stringify(m));
let seen = 0;
for (const k in m) {
  seen++;
}
console.log(seen);
console.log(Object.keys({ ...m }).length);
console.log(Object.prototype.toString.call(m));

// §13.5.1.2 step 5.d — a non-configurable own property refuses the
// delete, and module code is strict, so the refusal throws.
try {
  delete m.PI;
  console.log("no throw");
} catch (e) {
  console.log("threw");
}
console.log(m.PI === Math.PI);

// a function property is configurable, so it deletes for real
console.log(delete m.abs, typeof m.abs);
console.log(Math.max(1, 2));
